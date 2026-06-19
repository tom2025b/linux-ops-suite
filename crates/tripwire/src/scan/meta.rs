//! Turning a path into an [`Entry`]'s metadata, dependency-free. Uses
//! `std::fs::symlink_metadata` (an `lstat`, so a symlink is described as itself,
//! never followed) for kind/size/mtime, and `std::os::unix::fs::MetadataExt` for
//! the Unix mode/uid/gid bits. Content hashing for files is streamed in
//! [`hash_file`] so a large watched file never loads wholesale into memory.

use std::fs::{self, Metadata};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::hash::Sha256;
use crate::model::EntryKind;

/// Chunk size for streaming a file through the hasher. 64 KiB balances syscall
/// count against memory for the large-file case.
const HASH_CHUNK: usize = 64 * 1024;

/// The non-content metadata tripwire records for any path: its kind plus the
/// permission/owner/size/time bits. Content (hash, symlink target) is resolved
/// separately by the scanner because it depends on per-path options.
pub struct Meta {
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub mode: String,
    pub uid: u32,
    pub gid: u32,
    pub mtime: Option<String>,
}

impl Meta {
    /// Build from `symlink_metadata` (lstat) so a symlink is recorded as a
    /// symlink. `follow` is honored by the caller deciding which metadata to
    /// pass — see [`crate::scan`]; here we just describe what we're given.
    pub fn from_metadata(md: &Metadata) -> Self {
        let kind = if md.file_type().is_symlink() {
            EntryKind::Symlink
        } else if md.is_dir() {
            EntryKind::Dir
        } else if md.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };

        let size = if kind == EntryKind::File {
            Some(md.len())
        } else {
            None
        };

        Meta {
            kind,
            size,
            mode: format_mode(md.mode()),
            uid: md.uid(),
            gid: md.gid(),
            mtime: format_mtime(md),
        }
    }
}

/// Format the permission bits of a Unix mode as a 4-digit octal string
/// (`0644`), masking off the file-type bits — those are carried by `kind`, and
/// keeping them out of `mode` means a content edit that preserves permissions
/// never shows a spurious mode change.
pub fn format_mode(raw_mode: u32) -> String {
    format!("{:04o}", raw_mode & 0o7777)
}

/// Format a file's mtime as an RFC3339 UTC string without pulling in `chrono`.
/// Returns `None` if the time is unavailable or pre-epoch (we don't editorialize
/// on exotic clocks — mtime is informational only).
fn format_mtime(md: &Metadata) -> Option<String> {
    let modified = md.modified().ok()?;
    let dur = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(rfc3339_utc(dur.as_secs()))
}

/// Convert whole seconds since the Unix epoch to an `YYYY-MM-DDTHH:MM:SSZ`
/// string. A self-contained civil-date calculation (Howard Hinnant's
/// `days_from_civil` inverse) so there's no date dependency. Sub-second
/// precision is intentionally dropped — mtime here is for human context.
pub fn rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // days -> civil (y, m, d). Algorithm from chrono::naive / Hinnant.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Stream a file through SHA-256, returning its lowercase hex digest. `Err` (an
/// `io::Error`) means the file exists but couldn't be read — the caller turns
/// that into an `unreadable` entry, never a hard failure.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.hex())
}

/// Read a symlink's target as a string. `None` if it can't be read.
pub fn read_link_target(path: &Path) -> Option<String> {
    fs::read_link(path)
        .ok()
        .map(|t| t.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn format_mode_masks_type_bits() {
        // 0o100644 is a regular file with 0644 perms; type bits must drop.
        assert_eq!(format_mode(0o100_644), "0644");
        assert_eq!(format_mode(0o040_755), "0755");
        assert_eq!(format_mode(0o0600), "0600");
    }

    #[test]
    fn rfc3339_matches_known_instants() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        // 2026-06-18T00:00:00Z
        assert_eq!(rfc3339_utc(1_781_740_800), "2026-06-18T00:00:00Z");
    }

    #[test]
    fn meta_classifies_file_and_dir() {
        let dir = tempdir().unwrap();
        let fp = dir.path().join("f.txt");
        let mut f = fs::File::create(&fp).unwrap();
        f.write_all(b"hello").unwrap();

        let md = fs::symlink_metadata(&fp).unwrap();
        let m = Meta::from_metadata(&md);
        assert_eq!(m.kind, EntryKind::File);
        assert_eq!(m.size, Some(5));

        let dmd = fs::symlink_metadata(dir.path()).unwrap();
        let dm = Meta::from_metadata(&dmd);
        assert_eq!(dm.kind, EntryKind::Dir);
        assert_eq!(dm.size, None);
    }

    #[test]
    fn symlink_is_recorded_as_symlink_not_followed() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("real.txt");
        fs::write(&target, b"data").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let md = fs::symlink_metadata(&link).unwrap();
        assert_eq!(Meta::from_metadata(&md).kind, EntryKind::Symlink);
        assert_eq!(read_link_target(&link).as_deref(), target.to_str());
    }

    #[test]
    fn hash_file_matches_known_content() {
        let dir = tempdir().unwrap();
        let fp = dir.path().join("abc");
        fs::write(&fp, b"abc").unwrap();
        assert_eq!(
            hash_file(&fp).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
