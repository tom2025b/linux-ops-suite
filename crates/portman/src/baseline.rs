//! The baseline: a saved snapshot of listeners, and the diff of a live scan
//! against it. `portman baseline` writes one; `portman diff` reads it and
//! reports what appeared, what vanished, and which kept-open listeners changed
//! owner (a service replaced, a port quietly taken over by something else).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::PortmanError;
use crate::model::Listener;

/// The on-disk baseline. Versioned envelope so a future format change is
/// detectable rather than silently misread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub schema_version: u32,
    pub source_tool: String,
    /// The recorded listeners, in the order they were scanned.
    pub listeners: Vec<Listener>,
}

impl Baseline {
    const SCHEMA: u32 = 1;

    /// Wrap a freshly-scanned listener set as a baseline ready to save.
    pub fn from_scan(listeners: Vec<Listener>) -> Self {
        Baseline {
            schema_version: Self::SCHEMA,
            source_tool: "portman".to_string(),
            listeners,
        }
    }

    /// Write the baseline as pretty JSON to `path`, creating parent dirs.
    pub fn save(&self, path: &Path) -> Result<(), PortmanError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| PortmanError::SaveFailed {
                path: path.to_path_buf(),
                source,
            })?;
        }
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        fs::write(path, json).map_err(|source| PortmanError::SaveFailed {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Load a baseline from `path`, distinguishing "not recorded yet" from
    /// "recorded but corrupt" so the CLI can give the right next step, and
    /// rejecting a schema/source this build should not interpret.
    pub fn load(path: &Path) -> Result<Self, PortmanError> {
        if !path.exists() {
            return Err(PortmanError::NoBaseline {
                path: path.to_path_buf(),
            });
        }
        let text = fs::read_to_string(path).map_err(|e| PortmanError::BadBaseline {
            path: path.to_path_buf(),
            detail: e.to_string(),
        })?;
        let baseline: Baseline =
            serde_json::from_str(&text).map_err(|e| PortmanError::BadBaseline {
                path: path.to_path_buf(),
                detail: e.to_string(),
            })?;
        if baseline.schema_version > Self::SCHEMA {
            return Err(PortmanError::BadBaseline {
                path: path.to_path_buf(),
                detail: format!(
                    "baseline schema v{} is newer than this portman understands (v{}); upgrade portman or re-record",
                    baseline.schema_version,
                    Self::SCHEMA
                ),
            });
        }
        if baseline.source_tool != "portman" {
            return Err(PortmanError::BadBaseline {
                path: path.to_path_buf(),
                detail: format!(
                    "expected source_tool=portman, found {}",
                    baseline.source_tool
                ),
            });
        }
        Ok(baseline)
    }
}

/// One entry in a diff. Carries enough to render a line without re-looking-up
/// anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A listener present now that the baseline didn't have.
    Added(Listener),
    /// A listener in the baseline that's no longer listening.
    Removed(Listener),
    /// A listener on the same proto/addr/port whose owner changed (e.g. a
    /// different process now answers on it).
    OwnerChanged {
        key: String,
        was: String,
        now: String,
    },
}

/// The full result of comparing a live scan to a baseline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    pub changes: Vec<Change>,
}

impl Diff {
    /// Whether anything changed at all.
    pub fn is_clean(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Compare a live scan (`current`) against a recorded `baseline`. Matches
/// listeners by their stable key (proto/addr/port); same-key listeners with a
/// different owner chain become an `OwnerChanged`, not an add+remove pair.
pub fn diff(baseline: &[Listener], current: &[Listener]) -> Diff {
    let base_by_key: BTreeMap<String, &Listener> = baseline.iter().map(|l| (l.key(), l)).collect();
    let cur_by_key: BTreeMap<String, &Listener> = current.iter().map(|l| (l.key(), l)).collect();

    let mut changes = Vec::new();

    // Added + owner-changed: walk current, compare against baseline.
    for (key, cur) in &cur_by_key {
        match base_by_key.get(key) {
            None => changes.push(Change::Added((*cur).clone())),
            Some(base) => {
                if owner_fingerprint(base) != owner_fingerprint(cur) {
                    changes.push(Change::OwnerChanged {
                        key: key.clone(),
                        was: owner_summary(base),
                        now: owner_summary(cur),
                    });
                }
            }
        }
    }

    // Removed: in baseline but not current.
    for (key, base) in &base_by_key {
        if !cur_by_key.contains_key(key) {
            changes.push(Change::Removed((*base).clone()));
        }
    }

    Diff { changes }
}

fn owner_fingerprint(listener: &Listener) -> String {
    let o = &listener.owner;
    format!(
        "process={}|exe={}|unit={}|package={}",
        opt(&o.process),
        opt(&o.exe),
        opt(&o.unit),
        opt(&o.package)
    )
}

fn owner_summary(listener: &Listener) -> String {
    let o = &listener.owner;
    let mut parts = vec![listener.owner_label()];
    if let Some(unit) = &o.unit {
        parts.push(format!("unit={unit}"));
    }
    if let Some(package) = &o.package {
        parts.push(format!("pkg={package}"));
    }
    if let Some(exe) = &o.exe {
        parts.push(format!("exe={exe}"));
    }
    parts.join(" ")
}

fn opt(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Exposure, Owner, Proto};
    use tempfile::tempdir;

    fn listener(port: u16, process: &str) -> Listener {
        Listener {
            proto: Proto::Tcp,
            addr: "0.0.0.0".into(),
            port,
            exposure: Exposure::AllInterfaces,
            owner: Owner {
                process: Some(process.into()),
                ..Owner::unknown()
            },
        }
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sub/baseline.json");
        let b = Baseline::from_scan(vec![listener(22, "sshd")]);
        b.save(&path).expect("save");
        let loaded = Baseline::load(&path).expect("load");
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.listeners.len(), 1);
        assert_eq!(loaded.listeners[0].port, 22);
    }

    #[test]
    fn load_missing_is_no_baseline_not_corrupt() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert!(matches!(
            Baseline::load(&path),
            Err(PortmanError::NoBaseline { .. })
        ));
    }

    #[test]
    fn load_garbage_is_bad_baseline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(matches!(
            Baseline::load(&path),
            Err(PortmanError::BadBaseline { .. })
        ));
    }

    #[test]
    fn load_newer_schema_is_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("future.json");
        fs::write(
            &path,
            r#"{"schema_version":999,"source_tool":"portman","listeners":[]}"#,
        )
        .unwrap();
        assert!(matches!(
            Baseline::load(&path),
            Err(PortmanError::BadBaseline { .. })
        ));
    }

    #[test]
    fn load_wrong_source_tool_is_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tripwire.json");
        fs::write(
            &path,
            r#"{"schema_version":1,"source_tool":"tripwire","listeners":[]}"#,
        )
        .unwrap();
        assert!(matches!(
            Baseline::load(&path),
            Err(PortmanError::BadBaseline { .. })
        ));
    }

    #[test]
    fn load_current_schema_with_portman_source_is_accepted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("current.json");
        fs::write(
            &path,
            r#"{"schema_version":1,"source_tool":"portman","listeners":[]}"#,
        )
        .unwrap();

        let loaded = Baseline::load(&path).expect("current portman baseline should load");
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.source_tool, "portman");
        assert!(loaded.listeners.is_empty());
    }

    #[test]
    fn diff_detects_added_removed_and_owner_change() {
        let base = vec![listener(22, "sshd"), listener(80, "nginx")];
        // 80 now owned by a different process; 22 gone; 443 new.
        let current = vec![listener(443, "nginx"), listener(80, "apache2")];

        let d = diff(&base, &current);
        assert!(!d.is_clean());

        let has_added = d
            .changes
            .iter()
            .any(|c| matches!(c, Change::Added(l) if l.port == 443));
        let has_removed = d
            .changes
            .iter()
            .any(|c| matches!(c, Change::Removed(l) if l.port == 22));
        let has_owner = d.changes.iter().any(|c| {
            matches!(c, Change::OwnerChanged { was, now, .. } if was == "nginx" && now == "apache2")
        });
        assert!(has_added, "expected 443 added");
        assert!(has_removed, "expected 22 removed");
        assert!(has_owner, "expected 80 owner change");
    }

    #[test]
    fn identical_scan_is_clean() {
        let base = vec![listener(22, "sshd")];
        let current = vec![listener(22, "sshd")];
        assert!(diff(&base, &current).is_clean());
    }

    #[test]
    fn same_port_restart_same_owner_is_not_a_change() {
        // Same process name, new pid — must not register as a change.
        let mut base_l = listener(22, "sshd");
        base_l.owner.pid = Some(100);
        let mut cur_l = listener(22, "sshd");
        cur_l.owner.pid = Some(200);
        assert!(diff(&[base_l], &[cur_l]).is_clean());
    }

    #[test]
    fn pid_only_restart_with_full_owner_chain_is_clean() {
        let mut base_l = listener(443, "server");
        base_l.owner.pid = Some(100);
        base_l.owner.exe = Some("/usr/bin/server".into());
        base_l.owner.unit = Some("server.service".into());
        base_l.owner.package = Some("server".into());

        let mut cur_l = listener(443, "server");
        cur_l.owner.pid = Some(200);
        cur_l.owner.exe = Some("/usr/bin/server".into());
        cur_l.owner.unit = Some("server.service".into());
        cur_l.owner.package = Some("server".into());

        assert!(diff(&[base_l], &[cur_l]).is_clean());
    }

    #[test]
    fn same_process_name_with_different_chain_is_a_change() {
        let mut base_l = listener(443, "server");
        base_l.owner.exe = Some("/usr/bin/server".into());
        base_l.owner.unit = Some("server.service".into());
        base_l.owner.package = Some("server".into());

        let mut cur_l = listener(443, "server");
        cur_l.owner.exe = Some("/tmp/server".into());
        cur_l.owner.unit = Some("user-server.service".into());
        cur_l.owner.package = Some("local-build".into());

        let d = diff(&[base_l], &[cur_l]);
        assert!(d.changes.iter().any(|c| matches!(
            c,
            Change::OwnerChanged { was, now, .. }
                if was.contains("/usr/bin/server") && now.contains("/tmp/server")
        )));
    }

    #[test]
    fn unit_or_package_change_is_owner_drift_even_with_same_process_and_exe() {
        let mut base_l = listener(8443, "server");
        base_l.owner.exe = Some("/usr/bin/server".into());
        base_l.owner.unit = Some("server.service".into());
        base_l.owner.package = Some("server".into());

        let mut cur_l = listener(8443, "server");
        cur_l.owner.exe = Some("/usr/bin/server".into());
        cur_l.owner.unit = Some("server-alt.service".into());
        cur_l.owner.package = Some("server-alt".into());

        let d = diff(&[base_l], &[cur_l]);
        assert!(d.changes.iter().any(|c| matches!(
            c,
            Change::OwnerChanged { was, now, .. }
                if was.contains("unit=server.service")
                    && was.contains("pkg=server")
                    && now.contains("unit=server-alt.service")
                    && now.contains("pkg=server-alt")
        )));
    }
}
