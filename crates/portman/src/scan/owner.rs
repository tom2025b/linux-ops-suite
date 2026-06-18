//! Resolve a socket inode into its full ownership chain:
//! inode -> pid -> process/exe -> systemd unit -> package.
//!
//! Every link is best-effort. The pid step needs to read other processes'
//! `/proc/<pid>/fd`, which only root can do for processes it doesn't own — so a
//! non-root run resolves owners only for *its own* sockets and leaves the rest
//! `None`. That's the graceful-degradation contract: portman always lists the
//! socket, and tells you as much of the "why" as your privileges allow.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::model::Owner;

/// Map from socket inode to the pid that holds it open. Built once per scan by
/// walking `/proc/*/fd` — doing it once and indexing is far cheaper than a
/// per-socket filesystem walk.
pub struct InodeMap {
    by_inode: HashMap<u64, u32>,
}

impl InodeMap {
    /// Walk every readable `/proc/<pid>/fd` and record which pid owns each
    /// socket inode. Unreadable pids (permission, or the process exited mid
    /// walk) are silently skipped — that's the non-root degradation path.
    pub fn build() -> Self {
        let mut by_inode = HashMap::new();
        let Ok(entries) = fs::read_dir("/proc") else {
            return InodeMap { by_inode };
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue; // non-numeric /proc entry (e.g. "self", "net")
            };
            index_pid_fds(pid, &mut by_inode);
        }
        InodeMap { by_inode }
    }

    /// The pid owning a socket inode, if we could see it.
    fn pid_for(&self, inode: u64) -> Option<u32> {
        self.by_inode.get(&inode).copied()
    }
}

/// Add every socket inode held by `pid` to the map. Each fd is a symlink; the
/// ones pointing at sockets read as `socket:[<inode>]`.
fn index_pid_fds(pid: u32, by_inode: &mut HashMap<u64, u32>) {
    let fd_dir = format!("/proc/{pid}/fd");
    let Ok(fds) = fs::read_dir(&fd_dir) else {
        return; // not ours to read, or gone
    };
    for fd in fds.flatten() {
        if let Ok(target) = fs::read_link(fd.path()) {
            if let Some(inode) = socket_inode(&target.to_string_lossy()) {
                // First writer wins; a socket has one owning pid in practice.
                by_inode.entry(inode).or_insert(pid);
            }
        }
    }
}

/// Extract the inode from a `socket:[12345]` fd symlink target.
fn socket_inode(link: &str) -> Option<u64> {
    let rest = link.strip_prefix("socket:[")?;
    rest.strip_suffix(']')?.parse().ok()
}

/// Resolve the full chain for one socket inode. Returns [`Owner::unknown`] when
/// the inode couldn't be tied to a visible pid.
pub fn resolve(inode: u64, map: &InodeMap) -> Owner {
    let Some(pid) = map.pid_for(inode) else {
        return Owner::unknown();
    };

    let process = read_comm(pid);
    let exe = read_exe(pid);
    let unit = read_unit(pid);
    let package = exe.as_deref().and_then(package_for);

    Owner {
        pid: Some(pid),
        process,
        exe,
        unit,
        package,
    }
}

/// `/proc/<pid>/comm` — the short process name, trimmed.
fn read_comm(pid: u32) -> Option<String> {
    let s = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let name = s.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `/proc/<pid>/exe` — the resolved executable path, if readable.
fn read_exe(pid: u32) -> Option<String> {
    let target = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    Some(target.to_string_lossy().into_owned())
}

/// The systemd unit a pid belongs to, read from its cgroup line. Falling back to
/// the cgroup avoids shelling out to `systemctl` for the common case and works
/// even when `systemctl` isn't on PATH. Returns `None` when the pid isn't under
/// a `*.service`/`*.socket` slice (e.g. a bare daemon, or no systemd).
fn read_unit(pid: u32) -> Option<String> {
    let cgroup = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    unit_from_cgroup(&cgroup)
}

/// Pull a `*.service` / `*.socket` unit name out of a `/proc/<pid>/cgroup`
/// blob. systemd encodes the unit as the last `…/<unit>` path segment on the
/// unified (or name=systemd) hierarchy line. Pure for testability.
fn unit_from_cgroup(cgroup: &str) -> Option<String> {
    cgroup
        .lines()
        .filter_map(|line| line.rsplit('/').next())
        .map(str::trim)
        .find(|seg| seg.ends_with(".service") || seg.ends_with(".socket"))
        .map(|s| s.to_string())
}

/// Best-effort package owning an executable path. Tries dpkg, then rpm, then
/// pacman — whichever is present. A 5-second cap keeps one slow query from
/// stalling the whole scan. Returns `None` on any miss (unpackaged, no pkg
/// manager, timeout).
fn package_for(exe: &str) -> Option<String> {
    if let Some(p) = dpkg_owner(exe) {
        return Some(p);
    }
    if let Some(p) = rpm_owner(exe) {
        return Some(p);
    }
    pacman_owner(exe)
}

/// `dpkg -S <path>` -> `pkg: /path`. We take the package name before the colon.
fn dpkg_owner(exe: &str) -> Option<String> {
    let out = run("dpkg", &["-S", exe])?;
    out.split(':')
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `rpm -qf --queryformat %{NAME} <path>`.
fn rpm_owner(exe: &str) -> Option<String> {
    let out = run("rpm", &["-qf", "--queryformat", "%{NAME}", exe])?;
    let name = out.trim();
    (!name.is_empty() && !name.contains("not owned")).then(|| name.to_string())
}

/// `pacman -Qo <path>` -> `<path> is owned by <pkg> <ver>`.
fn pacman_owner(exe: &str) -> Option<String> {
    let out = run("pacman", &["-Qo", exe])?;
    // ".../sshd is owned by openssh 9.6p1-1" -> "openssh"
    let after = out.split("owned by").nth(1)?;
    after.split_whitespace().next().map(|s| s.to_string())
}

/// Run a command with a hard timeout, returning trimmed stdout on success only.
/// Any failure (missing binary, nonzero exit, timeout) is `None` — callers
/// treat that as "couldn't resolve", never as an error.
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    use std::time::{Duration, Instant};
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let out = child.wait_with_output().ok()?;
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return (!s.is_empty()).then_some(s);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
}

/// Read the inode of a path on disk (used by tests; the live join goes through
/// the fd symlink text, not this).
#[allow(dead_code)]
fn path_inode(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|m| m.ino())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_inode_parses_fd_target() {
        assert_eq!(socket_inode("socket:[12345]"), Some(12345));
        assert_eq!(socket_inode("anon_inode:[eventfd]"), None);
        assert_eq!(socket_inode("/dev/null"), None);
        assert_eq!(socket_inode("socket:[notanumber]"), None);
    }

    #[test]
    fn unit_from_cgroup_finds_service_and_socket() {
        let svc = "0::/system.slice/ssh.service\n";
        assert_eq!(unit_from_cgroup(svc).as_deref(), Some("ssh.service"));

        let sock = "0::/system.slice/cups.socket\n";
        assert_eq!(unit_from_cgroup(sock).as_deref(), Some("cups.socket"));

        // A user session scope is not a packaged service unit we name.
        let scope = "0::/user.slice/user-1000.slice/session-2.scope\n";
        assert_eq!(unit_from_cgroup(scope), None);
    }

    #[test]
    fn resolve_is_unknown_for_unseen_inode() {
        let map = InodeMap {
            by_inode: HashMap::new(),
        };
        let owner = resolve(424242, &map);
        assert!(!owner.is_known());
    }

    #[test]
    fn inode_map_build_does_not_panic() {
        // On Linux this finds our own sockets; on any platform it must not panic
        // and must at least be constructible.
        let _ = InodeMap::build();
    }

    #[test]
    fn path_inode_of_self_is_some() {
        // Smoke test for the metadata helper against a path that always exists.
        assert!(path_inode(Path::new("/proc/self/comm")).is_some());
    }
}
