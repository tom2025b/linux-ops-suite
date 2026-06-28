// `Path` is the borrowed, OS-agnostic filesystem-path type. We accept `&Path`
// (a borrow) rather than `String` so callers can pass a `&Path`, a `PathBuf`, or
// anything that derefs to `Path` without giving up ownership — the idiomatic Rust
// way to take "a path I only need to read."
use std::ffi::OsString;
// Bring `io` into scope so we can name the `io::Result` return type and wrap the
// (practically impossible) serialization-failure case via `io::Error::other`.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::model::snapshot::Snapshot;

/// Serialize `snapshot` to pretty JSON and publish it to `path`, creating the
/// parent directory if it does not yet exist.
///
/// PUBLISH STRATEGY depends on what `path` IS:
///   * A regular file (or a not-yet-existing path — the normal case): published
///     ATOMICALLY via a same-directory temp file + fsync + rename + directory
///     fsync, so `snapshot.json` never appears half-written to a reader.
///   * A NON-regular target (e.g. `/dev/stdout`, a fifo, a character/block
///     device): streamed DIRECTLY to the target. The atomic temp+rename dance is
///     impossible here — you cannot create a temp file next to `/dev/stdout` (its
///     parent is `/`) nor rename a regular file over a device and have the bytes
///     reach the stream — so for these targets we just write the bytes through.
///     This is what makes `workstate /dev/stdout | rexops` work.
///
/// PRETTY (not compact) JSON: `snapshot.json` is read by humans during
/// development and diffed in review, so readability beats a few saved bytes.
///
/// Returns `io::Result<()>`: `Ok(())` once the snapshot is published, or an
/// `io::Error` if creating the directory or writing the file fails (permissions,
/// disk full, a bad path, ...). The caller (main.rs) adds human context and exits.
///
/// SERIALIZATION ERROR HANDLING: `serde_json::to_string_pretty` returns its own
/// error type. For a concrete `Snapshot` (plain owned data, no exotic map keys)
/// this cannot realistically fail — but we do NOT `.unwrap()` it. Unwrapping would
/// turn a theoretical failure into a panic; instead we convert it into an
/// `io::Error` so the whole function has ONE honest error channel the caller can
/// handle uniformly. Honest error propagation over hidden panics.
pub fn write_snapshot(snapshot: &Snapshot, path: &Path) -> io::Result<()> {
    // 1. Turn the Snapshot into a JSON string. `snapshot` is already a `&Snapshot`;
    //    `to_string_pretty` borrows it (read-only). On the off chance serde errors,
    //    `io::Error::other` wraps it so this function exposes ONE error type. We pass
    //    the constructor directly to `map_err` (point-free) — `io::Error::other`
    //    takes exactly the serde error and returns the `io::Error` we want.
    let json = serde_json::to_string_pretty(snapshot).map_err(io::Error::other)?;

    // 2. Pick the publish strategy. A target that already exists and is NOT a
    //    regular file (a device, fifo, socket, ...) cannot be published by atomic
    //    rename, so stream straight to it. Everything else (a regular file, or a
    //    path that does not exist yet) takes the durable atomic path.
    if is_non_regular_target(path) {
        return write_stream(path, json.as_bytes());
    }

    // 3. Make sure the parent directory exists before writing into it. A bare filename
    //    has an empty parent (""); for syncing we treat that as the current directory.
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let sync_dir = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)?;
    }

    // 4. Write to a same-directory temp file, sync it, rename it over the target,
    //    then sync the directory so the rename itself is durable.
    let (temp_path, temp_file) = create_temp_file(path)?;
    let result = publish_temp_file(temp_file, &temp_path, path, sync_dir, json.as_bytes());
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result?;

    Ok(()) // file written successfully
}

/// True when `path` already exists AND is something other than a regular file
/// (a device like `/dev/stdout`, a fifo, a socket, ...). Such targets cannot be
/// published by the atomic temp+rename path and must be streamed to directly.
///
/// We use `fs::metadata` (stat, which FOLLOWS symlinks), NOT `symlink_metadata`
/// (lstat). This is deliberate: `/dev/stdout` is commonly a symlink to a pipe or
/// device, and we care about the type of the FINAL target (is it a regular file we
/// can rename over, or a stream we must write through?), not the fact that it's a
/// symlink. lstat would report "symlink" and we'd wrongly take the atomic path. A
/// path that does NOT exist yet returns `false` (it will become a brand-new regular
/// file via the atomic path), and any stat error is treated as "regular" so the
/// normal atomic path runs and surfaces the real error there.
fn is_non_regular_target(path: &Path) -> bool {
    match fs::metadata(path) {
        Ok(meta) => !meta.file_type().is_file(),
        Err(_) => false,
    }
}

/// Stream `bytes` directly to an existing non-regular `path` (no temp, no rename).
///
/// Opens the target for writing WITHOUT `create_new`/`truncate`: `/dev/stdout` and
/// friends already exist and are not truncatable, so we just open-for-write and
/// push the bytes. No `sync_all` here — fsync on a pipe/stdout is meaningless and
/// can error; flushing the write is the meaningful durability step for a stream.
fn write_stream(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(bytes)?;
    file.flush()
}

fn create_temp_file(path: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "snapshot path must include a file name",
        )
    })?;
    let parent = path.parent().unwrap_or_else(|| Path::new(""));

    let mut temp_name = OsString::from(file_name);
    temp_name.push(format!(".{}.tmp", Uuid::new_v4()));
    let temp_path = if parent.as_os_str().is_empty() {
        PathBuf::from(&temp_name)
    } else {
        parent.join(&temp_name)
    };

    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;

    Ok((temp_path, file))
}

fn publish_temp_file(
    mut temp_file: File,
    temp_path: &Path,
    final_path: &Path,
    sync_dir: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    temp_file.write_all(bytes)?;
    temp_file.sync_all()?;
    drop(temp_file);

    fs::rename(temp_path, final_path)?;
    sync_directory(sync_dir)
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

// =============================================================================
// Tests — round-trip the writer without a dev-dependency.
// =============================================================================
// We build a tiny `Snapshot` BY HAND (all three sections Missing — no feeds, no
// fixtures needed), write it under the OS temp dir, read it back, and confirm it
// deserializes. This proves the atomic publish + serialize + read loop is faithful.
// Using `std::env::temp_dir()` keeps the test off the project tree and needs no
// `tempfile` crate. We add the process id to paths so concurrent `cargo test` runs
// cannot collide on the same path.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::provenance::{FeedId, Section};
    use crate::model::snapshot::{Snapshot, SCHEMA_VERSION};
    use chrono::Utc;

    fn missing_snapshot() -> Snapshot {
        // Construct a minimal but valid Snapshot directly. `Section::missing(..)`
        // gives each section a Missing status with no data — the cheapest valid
        // section, and it exercises the same serde path a real section would.
        Snapshot::new(
            Utc::now(),
            Section::missing(FeedId("scriptvault".to_string())),
            Section::missing(FeedId("toolfoundry".to_string())),
            Section::missing(FeedId("bulwark".to_string())),
            Section::missing(FeedId("proto".to_string())),
        )
    }

    #[test]
    fn write_then_read_roundtrips_and_keeps_schema_version() {
        let snapshot = missing_snapshot();

        // A unique path in the OS temp dir (PID-suffixed to avoid collisions).
        let mut path = std::env::temp_dir();
        path.push(format!("workstate_writer_test_{}.json", std::process::id()));

        // Write it. `&path` coerces `PathBuf` -> `&Path` automatically (Deref).
        write_snapshot(&snapshot, &path).expect("writing the snapshot must succeed");

        // Read the bytes back and deserialize into a Snapshot — this is the same
        // operation RexOps performs on our output, so it tests the real contract.
        let text = std::fs::read_to_string(&path).expect("snapshot file must be readable");
        let parsed: Snapshot =
            serde_json::from_str(&text).expect("written snapshot must deserialize");

        // The contract assertion: the persisted contract version survived the
        // round-trip and equals the current SCHEMA_VERSION (5).
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.schema_version, 5);

        // Clean up the temp file. We ignore the result: a failed cleanup must not
        // fail the test (the assertions above already passed), and the OS will
        // reclaim temp files regardless.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn atomic_write_replaces_existing_snapshot_without_temp_leftovers() {
        let dir = std::env::temp_dir().join(format!(
            "workstate_writer_atomic_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test directory must be creatable");
        let path = dir.join("snapshot.json");
        std::fs::write(&path, "old snapshot").expect("old snapshot must be writable");

        write_snapshot(&missing_snapshot(), &path).expect("atomic write must succeed");

        let text = std::fs::read_to_string(&path).expect("snapshot file must be readable");
        let parsed: Snapshot =
            serde_json::from_str(&text).expect("written snapshot must deserialize");
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);

        let leftover_temp = std::fs::read_dir(&dir)
            .expect("test directory must be readable")
            .filter_map(Result::ok)
            .any(|entry| {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                file_name.starts_with("snapshot.json.") && file_name.ends_with(".tmp")
            });
        assert!(!leftover_temp, "atomic writer left a temp file behind");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    // A non-regular output target (here a Unix FIFO, standing in for `/dev/stdout`)
    // must be STREAMED to, not published by atomic rename. This pins the fix for the
    // documented `workstate /dev/stdout | rexops` flow: before the fix the writer
    // tried to create a temp file beside the target and rename over it, which fails
    // on a device/fifo. We make a fifo, drain it from a reader thread while
    // `write_snapshot` writes, and confirm the JSON came through intact.
    #[cfg(unix)]
    #[test]
    fn streams_to_non_regular_target_instead_of_renaming() {
        use std::io::Read;

        let fifo = std::env::temp_dir().join(format!(
            "workstate_writer_fifo_{}_{}",
            std::process::id(),
            // A nanosecond tag so repeated runs in one process never collide on the
            // fifo path (mkfifo fails if the path already exists).
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));

        // Create the fifo via mkfifo (no dev-dependency needed). Skip the test
        // gracefully if mkfifo is unavailable rather than failing spuriously.
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !made {
            eprintln!("skipping fifo test: mkfifo unavailable");
            return;
        }

        // Reader thread: opening a fifo for read blocks until a writer opens it, so
        // this must run concurrently with the write below.
        let reader_path = fifo.clone();
        let reader = std::thread::spawn(move || {
            let mut buf = String::new();
            File::open(&reader_path)
                .expect("fifo must open for reading")
                .read_to_string(&mut buf)
                .expect("reading the fifo must succeed");
            buf
        });

        // Confirm our classifier agrees this is a non-regular target.
        assert!(
            is_non_regular_target(&fifo),
            "a fifo must be detected as a non-regular target"
        );

        write_snapshot(&missing_snapshot(), &fifo).expect("streaming to a fifo must succeed");

        let text = reader.join().expect("reader thread must not panic");
        let parsed: Snapshot =
            serde_json::from_str(&text).expect("streamed snapshot must deserialize");
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);

        let _ = std::fs::remove_file(&fifo);
    }
}
