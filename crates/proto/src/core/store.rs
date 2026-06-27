use std::fs;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::core::error::ProtoError;
use crate::core::feed::{DEFAULT_FEED_CAP, WorkstateFeed};
use crate::core::session::Session;

// -----------------------------------------------------------------------------
// default_dir — resolve the default sessions directory.
// -----------------------------------------------------------------------------
// Tries, in order: $XDG_DATA_HOME/proto/sessions (suite convention) → then
// $HOME/.proto/sessions (the usual case) → then a relative ./.proto/sessions if
// neither env var is set. Resolving config at runtime and degrading (rather than
// panicking) on an unusual environment matches the suite's "missing is handled"
// stance — see Workstate's snapshot-path resolution for the same pattern.
pub fn default_dir() -> PathBuf {
    // 1. Suite convention: honor XDG_DATA_HOME when present.
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        // Guard against an empty value (set-but-blank): treat it as unset.
        if !xdg.is_empty() {
            return Path::new(&xdg).join("proto").join("sessions");
        }
    }
    // 2. Fallback: ~/.proto/sessions.
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join(".proto").join("sessions");
    }
    // 3. Last resort: relative, so the tool still runs with no HOME/XDG at all.
    PathBuf::from(".proto").join("sessions")
}

// -----------------------------------------------------------------------------
// feed_default_dir — resolve the default Workstate FEED directory.
// -----------------------------------------------------------------------------
// The suite's agreed feed location is `$XDG_DATA_HOME/workstate/feeds` (where
// Bulwark and ToolFoundry drop their feeds for Workstate to ingest), falling back
// to `~/.local/share/workstate/feeds` when XDG_DATA_HOME is unset — the standard
// XDG default. Same degrade-don't-panic ladder as `default_dir`, ending in a
// relative path so the tool still runs in an environment with no HOME at all.
// Note this is DELIBERATELY under `workstate/`, not `proto/`: the feed is for
// Workstate, so it lives where Workstate looks, alongside its sibling producers.
pub fn feed_default_dir() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return Path::new(&xdg).join("workstate").join("feeds");
    }
    if let Some(home) = std::env::var_os("HOME") {
        // The XDG default for $XDG_DATA_HOME is ~/.local/share.
        return Path::new(&home)
            .join(".local")
            .join("share")
            .join("workstate")
            .join("feeds");
    }
    PathBuf::from("workstate").join("feeds")
}

// -----------------------------------------------------------------------------
// save — persist a session, returning the path it was written to.
// -----------------------------------------------------------------------------
// Creates the sessions directory if needed, derives the filename from the
// session's id (see session_id), and pretty-prints the JSON so the audit record
// is human-readable as well as machine-readable.
pub fn save(dir: &Path, session: &Session) -> Result<PathBuf> {
    // create_dir_all is idempotent and also makes parent dirs (~/.proto).
    fs::create_dir_all(dir).map_err(|source| ProtoError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;

    let path = dir.join(format!("{}.json", session_id(session)));

    // Pretty JSON: a session is a contract export AND something a human may open.
    // Serializing our plain data types essentially can't fail, but we RETURN that
    // error (via the typed Serialize variant) rather than `.expect()`-panicking the
    // process on the one-in-a-million case — a library hands the caller the error.
    let json = serde_json::to_string_pretty(session).map_err(|source| ProtoError::Serialize {
        what: "session",
        source,
    })?;

    // A write failure (permissions, full disk) is now its own WriteFile variant —
    // the message says "could not write", which points at the right fix, rather
    // than the old ReadFile wording that misdescribed a write as a read.
    fs::write(&path, json).map_err(|source| ProtoError::WriteFile {
        path: path.clone(),
        source,
    })?;

    Ok(path)
}

// -----------------------------------------------------------------------------
// session_id — the stable id/filename-stem for a session.
// -----------------------------------------------------------------------------
// `<protocol_id>-<YYYYMMDDThhmmssZ>`. Built from `generated_at`, which `run`
// stamps just before saving, so the id reflects when the file was written.
pub fn session_id(session: &Session) -> String {
    let stamp = session.generated_at.format("%Y%m%dT%H%M%SZ");
    format!("{}-{}", session.protocol_id, stamp)
}

// -----------------------------------------------------------------------------
// SessionEntry — a lightweight listing row (loaded session + its id).
// -----------------------------------------------------------------------------
// `sessions`/`show` want both the parsed session and the id you'd type to open
// it, so we pair them. We keep the whole Session (sessions are small) rather than
// inventing a separate summary type — simplest thing that works.
pub struct SessionEntry {
    pub id: String,
    pub session: Session,
}

// -----------------------------------------------------------------------------
// list — every saved session in `dir`, newest first.
// -----------------------------------------------------------------------------
// Reads each *.json, parses it, and returns them sorted by id DESCENDING (ids
// are timestamped, so descending = newest first). A missing directory is treated
// as "no sessions yet" (empty Vec), NOT an error — you haven't run anything yet
// is a normal state, mirroring the suite's "missing file is handled" stance.
pub fn list(dir: &Path) -> Result<Vec<SessionEntry>> {
    // Not-yet-created store => no sessions. Don't error on a first run.
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let read = fs::read_dir(dir).map_err(|source| ProtoError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;

    let mut entries = Vec::new();
    for item in read {
        let path = item?.path();
        // Only consider .json files; ignore anything else dropped in the dir.
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // The id is the filename without extension.
        let id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => stem.to_string(),
            None => continue, // non-UTF8 name: skip rather than fail the listing
        };
        let session = load_path(&path)?;
        entries.push(SessionEntry { id, session });
    }

    // Newest first: ids embed a sortable timestamp, so reverse-lexicographic works.
    entries.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(entries)
}

// -----------------------------------------------------------------------------
// latest_run_by_protocol — when each protocol was most recently run.
// -----------------------------------------------------------------------------
// Maps protocol_id -> the newest `started_at` among that protocol's sessions.
// The picker uses this to annotate each protocol with "last run: 2d ago" so the
// operator can see at a glance which checklists are stale. A missing store is a
// normal empty map (nothing run yet), not an error.
//
// Because `list` already returns sessions NEWEST-FIRST, the FIRST time we see a
// given protocol_id is its most recent run — so we only insert if absent.
pub fn latest_run_by_protocol(
    dir: &Path,
) -> Result<std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>> {
    use std::collections::HashMap;
    let mut latest: HashMap<String, chrono::DateTime<chrono::Utc>> = HashMap::new();
    for entry in list(dir)? {
        // entry_or_insert keeps the FIRST (newest, since list is sorted) seen.
        latest
            .entry(entry.session.protocol_id.clone())
            .or_insert(entry.session.started_at);
    }
    Ok(latest)
}

// -----------------------------------------------------------------------------
// build_feed — compile a WorkstateFeed from the sessions in `sessions_dir`.
// -----------------------------------------------------------------------------
// Reads every saved session (newest first, via `list`) and projects the most
// recent `DEFAULT_FEED_CAP` of them into the feed model. This is the bridge
// between the on-disk session store and the in-memory feed: the store knows WHERE
// sessions live; the feed model (feed.rs) knows their SHAPE. A missing store is a
// normal "no sessions yet" empty feed, not an error (same stance as `list`).
pub fn build_feed(sessions_dir: &Path) -> Result<WorkstateFeed> {
    let entries = list(sessions_dir)?; // newest-first, possibly empty
    // Pair each id with its session, as the model's constructor expects. We map to
    // borrowed refs so no session is cloned just to summarize it.
    let feed = WorkstateFeed::from_sessions(
        entries.iter().map(|e| (e.id.as_str(), &e.session)),
        DEFAULT_FEED_CAP,
    );
    Ok(feed)
}

// -----------------------------------------------------------------------------
// save_feed — write the feed to `<feed_dir>/proto.json`, returning the path.
// -----------------------------------------------------------------------------
// The filename is fixed (`proto.json`) because it's a ROLLING feed Workstate
// looks up by producer name — one well-known file, overwritten each time, exactly
// like `bulwark.json`/`toolfoundry.json`. Creates the feed dir if needed and
// pretty-prints the JSON (a human may open it too). Uses the WriteFile/Serialize
// error variants so a permission or disk failure reads honestly.
pub fn save_feed(feed_dir: &Path, feed: &WorkstateFeed) -> Result<PathBuf> {
    fs::create_dir_all(feed_dir).map_err(|source| ProtoError::WriteFile {
        path: feed_dir.to_path_buf(),
        source,
    })?;

    let path = feed_dir.join("proto.json");

    let json = serde_json::to_string_pretty(feed).map_err(|source| ProtoError::Serialize {
        what: "workstate feed",
        source,
    })?;

    fs::write(&path, json).map_err(|source| ProtoError::WriteFile {
        path: path.clone(),
        source,
    })?;

    Ok(path)
}

// -----------------------------------------------------------------------------
// load — one session by its id, or NotFound.
// -----------------------------------------------------------------------------
// Reuses the NotFound variant (it carries an id) so `show <bad-id>` reports the
// same shape as `run <bad-protocol>` — one error vocabulary for "no such thing".
pub fn load(dir: &Path, id: &str) -> Result<Session> {
    let path = dir.join(format!("{id}.json"));
    if !path.exists() {
        return Err(ProtoError::NotFound { id: id.to_string() });
    }
    load_path(&path)
}

// -----------------------------------------------------------------------------
// delete — remove one session file by its id, or NotFound.
// -----------------------------------------------------------------------------
// Used by `proto delete <id>`. Reuses the NotFound vocabulary (a missing id is
// the same "no such thing" as elsewhere) and the WriteFile variant for a removal
// that fails (a delete is a write to the filesystem). The CALLER is responsible
// for regenerating the feed afterwards, since deleting a session can change it.
pub fn delete(dir: &Path, id: &str) -> Result<()> {
    let path = dir.join(format!("{id}.json"));
    if !path.exists() {
        return Err(ProtoError::NotFound { id: id.to_string() });
    }
    fs::remove_file(&path).map_err(|source| ProtoError::WriteFile { path, source })
}

// -----------------------------------------------------------------------------
// load_path — read + parse one session file (internal helper).
// -----------------------------------------------------------------------------
fn load_path(path: &Path) -> Result<Session> {
    let text = fs::read_to_string(path).map_err(|source| ProtoError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    // A corrupt/incompatible session file is malformed INPUT, not a rule
    // violation, so it surfaces as ParseJson (carrying the path + serde_json's
    // line/column detail) — the JSON sibling of ParseYaml. The old code filed
    // this under Validation, which wrongly read as "the data is out of policy".
    serde_json::from_str(&text).map_err(|source| ProtoError::ParseJson {
        path: path.to_path_buf(),
        source,
    })
}
