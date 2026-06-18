//! Core types. A [`Listener`] is one listening socket plus everything portman
//! could resolve about *why* it is open — the ownership chain from the socket
//! down to the package that shipped the process. The check functions never
//! print; the human and JSON renderers derive everything they show from these.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Transport protocol of a listening socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Proto {
    Tcp,
    Tcp6,
    Udp,
    Udp6,
}

impl Proto {
    /// Short lowercase tag used in output and the baseline key (`tcp`, `udp6`).
    pub fn tag(self) -> &'static str {
        match self {
            Proto::Tcp => "tcp",
            Proto::Tcp6 => "tcp6",
            Proto::Udp => "udp",
            Proto::Udp6 => "udp6",
        }
    }

    /// Whether this is a TCP variant (UDP has no LISTEN state, so the "is it
    /// actually listening" rule differs per family).
    pub fn is_tcp(self) -> bool {
        matches!(self, Proto::Tcp | Proto::Tcp6)
    }
}

impl fmt::Display for Proto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// How exposed a listener is, derived from the bind address. Drives the one bit
/// of editorializing portman does: a service bound to a public/all-interfaces
/// address is worth more attention than one bound to loopback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Exposure {
    /// Bound to loopback only (`127.0.0.1`, `::1`).
    Loopback,
    /// Bound to a specific routable interface address.
    Interface,
    /// Bound to all interfaces (`0.0.0.0`, `::`) — reachable from anywhere the
    /// host is.
    AllInterfaces,
}

impl Exposure {
    /// Classify a bind address string. Anything we don't recognize as loopback
    /// or wildcard is treated as a concrete interface.
    pub fn classify(addr: &str) -> Exposure {
        match addr {
            "0.0.0.0" | "::" | "*" => Exposure::AllInterfaces,
            a if a == "127.0.0.1" || a.starts_with("127.") || a == "::1" => Exposure::Loopback,
            _ => Exposure::Interface,
        }
    }

    /// Short tag for human output.
    pub fn tag(self) -> &'static str {
        match self {
            Exposure::Loopback => "local",
            Exposure::Interface => "iface",
            Exposure::AllInterfaces => "PUBLIC",
        }
    }
}

/// The ownership chain for one socket: every link portman could resolve, from
/// the kernel socket down to the package. Each link is best-effort — a `None`
/// means "couldn't resolve" (not root, no systemd, no package manager), never
/// an error. That graceful degradation is the whole point of the design.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    /// Owning process id, from `/proc/*/fd` -> socket inode. `None` when the
    /// socket's inode couldn't be matched to a pid (typically: not root, and
    /// the socket isn't ours).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Process command name (`/proc/<pid>/comm`), e.g. `sshd`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    /// Resolved executable path (`/proc/<pid>/exe`), e.g. `/usr/sbin/sshd`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
    /// The systemd unit the pid belongs to, e.g. `ssh.service`. `None` when not
    /// under systemd or `systemctl` isn't available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The package that owns the executable, e.g. `openssh-server`. `None` when
    /// no package manager could be queried or the file is unpackaged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

impl Owner {
    /// An entirely-unresolved owner (nothing learned about this socket).
    pub fn unknown() -> Self {
        Owner::default()
    }

    /// Whether any link of the chain was resolved at all.
    pub fn is_known(&self) -> bool {
        self.pid.is_some()
            || self.process.is_some()
            || self.unit.is_some()
            || self.package.is_some()
    }
}

/// One listening socket and its ownership chain — portman's unit of output and
/// the unit a baseline records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listener {
    pub proto: Proto,
    /// Bind address as text (`0.0.0.0`, `::1`, `192.168.1.10`).
    pub addr: String,
    pub port: u16,
    pub exposure: Exposure,
    pub owner: Owner,
}

impl Listener {
    /// Stable identity of a listener for baseline comparison: proto + bind
    /// address + port. The owner is deliberately *excluded* — a service
    /// restarting under a new pid is the same listener, not a new one. The diff
    /// reports owner *changes* separately for listeners that match on key.
    pub fn key(&self) -> String {
        format!("{}/{}:{}", self.proto.tag(), self.addr, self.port)
    }

    /// Best human label for who owns this: process name, else unit, else "?".
    pub fn owner_label(&self) -> String {
        if let Some(p) = &self.owner.process {
            p.clone()
        } else if let Some(u) = &self.owner.unit {
            u.clone()
        } else {
            "?".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_classifies_wildcard_and_loopback() {
        assert_eq!(Exposure::classify("0.0.0.0"), Exposure::AllInterfaces);
        assert_eq!(Exposure::classify("::"), Exposure::AllInterfaces);
        assert_eq!(Exposure::classify("127.0.0.1"), Exposure::Loopback);
        assert_eq!(Exposure::classify("127.0.0.53"), Exposure::Loopback);
        assert_eq!(Exposure::classify("::1"), Exposure::Loopback);
        assert_eq!(Exposure::classify("192.168.1.10"), Exposure::Interface);
    }

    #[test]
    fn proto_tcp_family_predicate() {
        assert!(Proto::Tcp.is_tcp());
        assert!(Proto::Tcp6.is_tcp());
        assert!(!Proto::Udp.is_tcp());
        assert!(!Proto::Udp6.is_tcp());
    }

    #[test]
    fn key_excludes_owner_so_a_restart_is_the_same_listener() {
        let base = Listener {
            proto: Proto::Tcp,
            addr: "0.0.0.0".into(),
            port: 22,
            exposure: Exposure::AllInterfaces,
            owner: Owner {
                pid: Some(100),
                process: Some("sshd".into()),
                ..Owner::unknown()
            },
        };
        let mut restarted = base.clone();
        restarted.owner.pid = Some(200); // same service, new pid
        assert_eq!(base.key(), restarted.key());
    }

    #[test]
    fn owner_label_falls_back_through_the_chain() {
        let mut l = Listener {
            proto: Proto::Udp,
            addr: "::".into(),
            port: 53,
            exposure: Exposure::AllInterfaces,
            owner: Owner::unknown(),
        };
        assert_eq!(l.owner_label(), "?");
        l.owner.unit = Some("systemd-resolved.service".into());
        assert_eq!(l.owner_label(), "systemd-resolved.service");
        l.owner.process = Some("systemd-resolve".into());
        assert_eq!(l.owner_label(), "systemd-resolve");
    }

    #[test]
    fn owner_is_known_only_when_a_link_resolved() {
        assert!(!Owner::unknown().is_known());
        let o = Owner {
            package: Some("openssh-server".into()),
            ..Owner::unknown()
        };
        assert!(o.is_known());
    }
}
