use std::collections::HashSet; // for detecting duplicate step ids in O(n)
use std::fs; // filesystem reads (read_to_string, read_dir)
use std::path::{Path, PathBuf}; // borrowed + owned filesystem paths

use crate::core::error::ProtoError;
use crate::core::protocol::Protocol;

// `crate::Result<T>` is our alias for `Result<T, ProtoError>` (defined in lib.rs).
use crate::Result;

// -----------------------------------------------------------------------------
// discover — list candidate protocol files in `dir`.
// -----------------------------------------------------------------------------
// Returns the paths of every *.yaml / *.yml file directly inside `dir`, sorted
// for stable, predictable output. A missing/unreadable directory is an ERROR
// here (the caller asked us to look in a specific place); higher layers decide
// whether that's fatal. We do NOT recurse — protocols live flat in one folder,
// the simplest layout that works.
pub fn discover(dir: &Path) -> Result<Vec<PathBuf>> {
    // read_dir fails if the path is missing or not a directory; wrap that into a
    // path-carrying ReadDir error so the message can name the offending folder.
    let entries = fs::read_dir(dir).map_err(|source| ProtoError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;

    let mut files = Vec::new(); // collect matching paths, then sort at the end
    for entry in entries {
        // Each iterator item is itself a Result (a single entry can fail to
        // stat); surface that as a generic Io error via `?` and the From impl.
        let entry = entry?;
        let path = entry.path();

        // Keep only files whose extension is yaml or yml. `extension()` returns
        // an Option<&OsStr>; we compare case-insensitively against both spellings.
        let is_yaml = path
            .extension()
            .and_then(|ext| ext.to_str()) // OsStr -> Option<&str>
            .map(|ext| ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml"))
            .unwrap_or(false); // no extension / non-UTF8 => not a protocol

        if is_yaml {
            files.push(path);
        }
    }

    // Sort so `proto list` order is deterministic regardless of filesystem order.
    files.sort();
    Ok(files)
}

// -----------------------------------------------------------------------------
// load_file — read + parse ONE protocol file (no validation yet).
// -----------------------------------------------------------------------------
// Reads the file to a string and parses it into a Protocol. Both failure modes
// carry the path: a read failure becomes ReadFile, a parse failure ParseYaml
// (which already includes serde_yaml's line/column detail).
pub fn load_file(path: &Path) -> Result<Protocol> {
    // Read the whole file. Small text files, so read_to_string is fine — no
    // need to stream. Wrap io errors with the path for a useful message.
    let text = fs::read_to_string(path).map_err(|source| ProtoError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;

    // Parse YAML into our Protocol struct. serde_yaml maps keys->fields and
    // errors if the shape is wrong (missing `id`, wrong type, an unknown `kind:`).
    // serde_yaml's own message already names the bad key/line and, for `kind`,
    // lists the accepted variants — so wrapping it in ParseYaml (which appends
    // `: {source}`) gives the author the full detail with the file path attached.
    let protocol: Protocol =
        serde_yaml::from_str(&text).map_err(|source| ProtoError::ParseYaml {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(protocol)
}

// -----------------------------------------------------------------------------
// check — the full per-file check: content rules AND the filename/id agreement.
// -----------------------------------------------------------------------------
// `validate` judges a Protocol in isolation; `check_stem` needs the path it came
// from. This pairs them so every caller that has both (load_all, the `validate`
// command) runs the SAME complete check, and neither forgets the stem rule.
//
// We run the stem check FIRST when the id is a usable slug, so a real
// filename/id mismatch is reported rather than masked by an unrelated content
// problem. When the id itself is empty/non-slug the stem comparison would be
// noise (an empty id "doesn't match" everything), so we defer to `validate`,
// which names the id problem precisely.
pub fn check(path: &Path, protocol: &Protocol) -> Result<()> {
    if is_slug(&protocol.id) {
        check_stem(path, protocol)?;
    }
    validate(protocol)
}

fn check_stem(path: &Path, protocol: &Protocol) -> Result<()> {
    // file_stem strips the extension; to_str may fail on a non-UTF8 name, in
    // which case we can't meaningfully compare, so we don't block the load.
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        && stem != protocol.id
    {
        return Err(ProtoError::Validation {
            id: protocol.id.clone(),
            reason: format!(
                "protocol `id` '{}' does not match its filename '{}.yaml' \
                 (rename one so they agree — Proto looks protocols up by id)",
                protocol.id, stem
            ),
        });
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// is_slug — does this string look like a stable machine id?
// -----------------------------------------------------------------------------
// A protocol/step id is typed at the command line (`proto run <id>`), keyed on in
// sessions, and (for protocols) expected to match the filename stem. To keep all
// of that frictionless we require a SLUG: lowercase ascii letters, digits, and
// single hyphens — no spaces, no uppercase, no punctuation. This catches the
// common authoring slips (a title accidentally pasted into `id`, a capitalised id
// that won't match the file) early, with a precise message, instead of at run
// time. Empty is rejected by the caller separately, so we only judge SHAPE here.
fn is_slug(s: &str) -> bool {
    !s.is_empty()
        // Every char must be a lowercase letter, a digit, or a hyphen.
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        // No leading/trailing hyphen and no "--" run — keeps ids tidy and
        // unambiguous (a trailing hyphen reads like a typo, "a--b" like a mistake).
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

// -----------------------------------------------------------------------------
// validate — enforce the SEMANTIC rules on a parsed Protocol.
// -----------------------------------------------------------------------------
// Returns Ok(()) if the protocol is internally consistent, or a single
// ProtoError::Validation whose `reason` lists EVERY rule the protocol breaks.
// Collecting all violations (rather than returning on the first) means an author
// fixing a new protocol sees the whole list in one `proto validate`, not one
// error per re-run. Rules:
//   1. `id` is non-empty and a slug ([a-z0-9-], no leading/trailing/double `-`).
//   2. `title` is non-empty (the run needs a heading).
//   3. a present `version:` is non-blank (an empty value is an authoring slip).
//   4. there is at least one step (an empty checklist is meaningless).
//   5. every step has a non-empty slug `id` and a non-empty `title`.
//   6. step ids are unique within the protocol (sessions key on them).
pub fn validate(protocol: &Protocol) -> Result<()> {
    // Accumulate every problem here; we only build an error if it's non-empty.
    let mut problems: Vec<String> = Vec::new();

    // Rule 1: top-level id must be present AND a clean slug.
    if protocol.id.trim().is_empty() {
        problems.push("protocol `id` is empty".to_string());
    } else if !is_slug(&protocol.id) {
        problems.push(format!(
            "protocol `id` '{}' is not a slug (use lowercase letters, digits, and single hyphens)",
            protocol.id
        ));
    }

    // Rule 2: a human-facing heading is required.
    if protocol.title.trim().is_empty() {
        problems.push("protocol `title` is empty".to_string());
    }

    // Rule 3: `version` is optional, but if the key is present it must say
    // something. A bare `version:` (empty string) is almost always a mistake.
    if !protocol.version.is_empty() && protocol.version.trim().is_empty() {
        problems.push("protocol `version` is present but blank".to_string());
    }

    // Rule 4: a checklist with no items is a bug, not a valid protocol.
    if protocol.steps.is_empty() {
        problems.push("protocol has no steps".to_string());
    }

    // Rules 5 + 6: per-step checks. `seen` tracks ids to catch duplicates.
    let mut seen: HashSet<&str> = HashSet::new();
    for (index, step) in protocol.steps.iter().enumerate() {
        // Human-friendly position (1-based) for error messages.
        let pos = index + 1;

        if step.id.trim().is_empty() {
            problems.push(format!("step #{pos} has an empty `id`"));
            // Skip the rest of this step's checks: with no id we can't name it
            // sensibly, and it can't meaningfully collide in `seen`.
            continue;
        }
        if !is_slug(&step.id) {
            problems.push(format!(
                "step '{}' (#{pos}) has a non-slug `id` (use lowercase letters, digits, and single hyphens)",
                step.id
            ));
        }
        if step.title.trim().is_empty() {
            problems.push(format!("step '{}' has an empty `title`", step.id));
        }
        // `insert` returns false if the value was already present => duplicate.
        if !seen.insert(step.id.as_str()) {
            problems.push(format!("duplicate step id '{}'", step.id));
        }
    }

    // No problems => valid. Otherwise build ONE error carrying the whole list.
    if problems.is_empty() {
        return Ok(());
    }

    // The id label falls back to "<unknown>" when even the id is blank, so the
    // error still has a subject. One reason per line, bullet-prefixed, so a
    // multi-failure protocol reads as a tidy checklist of what to fix.
    let id_label = if protocol.id.trim().is_empty() {
        "<unknown>".to_string()
    } else {
        protocol.id.clone()
    };
    let reason = if problems.len() == 1 {
        problems.remove(0)
    } else {
        // Lead with a count, then bullet each problem on its own indented line.
        let mut joined = format!("{} problems:", problems.len());
        for p in &problems {
            joined.push_str("\n  - ");
            joined.push_str(p);
        }
        joined
    };
    Err(ProtoError::Validation {
        id: id_label,
        reason,
    })
}

// -----------------------------------------------------------------------------
// load_all — discover + load + validate every protocol in `dir`.
// -----------------------------------------------------------------------------
// The one-stop function the CLI uses for `list`. Returns only protocols that
// load AND validate; the first failure short-circuits with its precise error,
// so `proto list` against a broken folder tells you exactly what's wrong rather
// than silently dropping files.
pub fn load_all(dir: &Path) -> Result<Vec<Protocol>> {
    let mut protocols = Vec::new();
    for path in discover(dir)? {
        let protocol = load_file(&path)?;
        check(&path, &protocol)?; // content rules + id-matches-filename
        protocols.push(protocol);
    }
    Ok(protocols)
}

// -----------------------------------------------------------------------------
// find — locate a single validated protocol by id within `dir`.
// -----------------------------------------------------------------------------
// Used by `run` and the interactive picker's chosen protocol. It resolves and
// validates ONLY the requested protocol (via `find_one`), so a malformed or
// invalid UNRELATED sibling in the same directory can't stop you running a good
// one. Returns the validated Protocol, the requested protocol's own
// Validation/ParseYaml error if IT is broken, or NotFound.
//
// (This used to load+validate the WHOLE directory, which meant one typo in any
// sibling file bricked `run` for every protocol — a reliability foot-gun. `run`
// only needs the file it's about to walk to be sound.)
pub fn find(dir: &Path, id: &str) -> Result<Protocol> {
    find_one(dir, id)
}

// -----------------------------------------------------------------------------
// find_one — locate + validate ONLY the protocol with `id`, ignoring siblings.
// -----------------------------------------------------------------------------
// The single-file counterpart to `find`/`load_all`. Used by `proto validate
// <id>`, where the contract is "tell me about THIS protocol" — so a malformed or
// invalid OTHER file in the same directory must NOT influence the result.
//
// Two phases, both sibling-agnostic:
//   1. STEM FAST-PATH: by convention `id` equals the filename stem, so we try
//      `<id>.yaml` then `<id>.yml` directly. If present, we load + `check` just
//      that file — we never even open the others.
//   2. FALLBACK SCAN: if no stem match (a file whose `id:` disagrees with its
//      name — itself a validation error we still want to surface), we scan the
//      directory and parse each candidate, SKIPPING any that fail to parse (a
//      broken sibling), looking for one whose parsed `id` matches. The first
//      match's `check` result is returned; checking it will report the stem
//      mismatch. No matching file anywhere => NotFound.
//
// Returns the validated Protocol on success, the file's Validation/ParseYaml
// error if the REQUESTED protocol is the one that's broken, or NotFound.
pub fn find_one(dir: &Path, id: &str) -> Result<Protocol> {
    // Phase 1: the file named after the id, in either YAML spelling. This is the
    // overwhelmingly common case and reads no sibling files at all.
    for ext in ["yaml", "yml"] {
        let candidate = dir.join(format!("{id}.{ext}"));
        if candidate.is_file() {
            let protocol = load_file(&candidate)?;
            check(&candidate, &protocol)?; // content rules + stem agreement
            return Ok(protocol);
        }
    }

    // Phase 2: no file is named `<id>.*`, so the protocol (if it exists) lives in
    // a file whose stem disagrees with its `id`. Scan to find it, tolerating
    // siblings that won't parse — they're not what we were asked about.
    for path in discover(dir)? {
        // A sibling that fails to parse is irrelevant to THIS lookup: skip it
        // rather than letting its error mask the protocol we're after.
        let Ok(protocol) = load_file(&path) else {
            continue;
        };
        if protocol.id == id {
            // Found it. `check` will (correctly) flag the stem mismatch that put
            // it here — that's a real problem with THIS protocol, worth reporting.
            check(&path, &protocol)?;
            return Ok(protocol);
        }
    }

    Err(ProtoError::NotFound { id: id.to_string() })
}
