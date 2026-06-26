//! The core state model and its freshness rules.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A snapshot older than this (in seconds) is considered stale.
pub const STALE_AFTER_SECS: u64 = 24 * 60 * 60;

/// The suite's recorded state at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workstate {
    /// When the snapshot was built, as Unix seconds.
    pub built_at: u64,
    /// The tools the suite knows about.
    #[serde(default)]
    pub tools: Vec<Tool>,
    /// Outstanding findings.
    #[serde(default)]
    pub findings: Vec<Finding>,
}

/// One tool and its last-known status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub status: String,
}

/// One flagged item needing attention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: String,
    pub reason: String,
}

impl Workstate {
    /// A fresh, empty snapshot stamped now.
    pub fn empty() -> Self {
        Workstate {
            built_at: now_secs(),
            tools: Vec::new(),
            findings: Vec::new(),
        }
    }

    /// Stamp the snapshot as built now.
    pub fn restamp(&mut self) {
        self.built_at = now_secs();
    }

    /// How long ago the snapshot was built, in seconds.
    pub fn age_secs(&self) -> u64 {
        now_secs().saturating_sub(self.built_at)
    }

    /// Whether the snapshot has aged past the freshness window.
    pub fn is_stale(&self) -> bool {
        self.age_secs() > STALE_AFTER_SECS
    }

    /// The finding with this id, if any.
    pub fn finding(&self, id: &str) -> Option<&Finding> {
        self.findings.iter().find(|f| f.id == id)
    }
}

/// The current time as Unix seconds (0 if the clock is before the epoch).
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
