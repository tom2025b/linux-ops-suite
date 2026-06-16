//! SHA256 verification: download a checksum, parse it, and compare it against
//! the freshly downloaded archive before anything is extracted or installed.
//! Fails closed.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::error::InstallError;
use crate::fs_ops::download_asset;
use crate::net::run_command;
use crate::release::{ReleaseAsset, ReleaseAssets};
use crate::ui::{detail, ok, step, warn};
use crate::Cli;

/// Verify the downloaded archive against its published SHA256 checksum before
/// it is ever extracted or made executable. Fails closed on mismatch. When no
/// checksum asset exists, this fails closed too unless `--allow-unverified`
/// is set; `--no-verify` skips the check entirely.
pub(crate) fn verify_download(
    cli: &Cli,
    assets: &ReleaseAssets,
    asset: &ReleaseAsset,
    downloaded: &Path,
    temp_dir: &Path,
) -> Result<(), InstallError> {
    if cli.no_verify {
        warn(format!(
            "{}: SHA256 verification skipped (--no-verify)",
            asset.name
        ));
        return Ok(());
    }

    let Some(checksum_asset) = assets.checksum_for(&asset.name) else {
        if !cli.allow_unverified {
            return Err(InstallError::ChecksumMissing {
                asset: asset.name.clone(),
            });
        }
        warn(format!(
            "{}: no published SHA256 checksum found; installing unverified",
            asset.name
        ));
        detail("missing checksum allowed because --allow-unverified was passed");
        return Ok(());
    };

    step(format!(
        "Verifying {} against {}",
        asset.name, checksum_asset.name
    ));

    let checksum_path = temp_dir.join(&checksum_asset.name);
    download_asset(checksum_asset, &checksum_path)?;

    let expected = read_expected_sha256(&checksum_path, &asset.name)?;
    let actual = sha256_of_file(downloaded)?;

    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(InstallError::ChecksumMismatch {
            asset: asset.name.clone(),
            expected,
            actual,
        });
    }

    ok(format!("{} checksum OK", asset.name));
    Ok(())
}

/// Parse a checksum file. Supports both a bare hex digest and the standard
/// `sha256sum` line format (`<hex>  <filename>`), including multi-line
/// `SHA256SUMS` manifests where we select the line matching `archive_name`.
///
/// A filename match always wins. A bare (filename-less) digest is only trusted
/// as a fallback when the file contains *exactly one* digest line total — in a
/// multi-entry manifest a bare line is ambiguous (we can't tell which asset it
/// belongs to), so we refuse it rather than guess.
pub(crate) fn read_expected_sha256(
    checksum_path: &Path,
    archive_name: &str,
) -> Result<String, InstallError> {
    let contents = fs::read_to_string(checksum_path).map_err(|source| InstallError::Io {
        context: format!("read checksum {}", checksum_path.display()),
        source,
    })?;

    let asset_name = checksum_name(checksum_path);

    // Scan every non-empty line. A filename match short-circuits; otherwise we
    // track how many digests we saw and the last bare one, so we can apply the
    // single-digest fallback only when it is unambiguous.
    let mut digest_count: usize = 0;
    let mut lone_bare_digest: Option<String> = None;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(digest) = parts.next() else {
            continue;
        };
        if !is_hex_sha256(digest) {
            continue;
        }
        digest_count += 1;
        // `sha256sum` separates digest and filename with two spaces; the
        // filename may be prefixed with `*` for binary mode.
        let file = parts.next().map(|name| name.trim_start_matches('*'));
        match file {
            Some(name) if name == archive_name => return Ok(digest.to_ascii_lowercase()),
            None => lone_bare_digest = Some(digest.to_ascii_lowercase()),
            Some(_) => {}
        }
    }

    // Only trust a bare digest if it is the *sole* digest in the file.
    lone_bare_digest
        .filter(|_| digest_count == 1)
        .ok_or_else(|| InstallError::ChecksumMalformed {
            asset: asset_name,
            detail: format!("no SHA256 digest for {archive_name} found in checksum file"),
        })
}

fn checksum_name(path: &Path) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("checksum")
        .to_string()
}

fn is_hex_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Compute the SHA256 of a file by shelling out to `sha256sum` (coreutils),
/// matching the dependency-free, curl/tar shell-out style of this installer.
fn sha256_of_file(path: &Path) -> Result<String, InstallError> {
    let output = run_command(Command::new("sha256sum").arg(path))?;
    let text = String::from_utf8_lossy(&output);
    let digest = text
        .split_whitespace()
        .next()
        .map(str::to_ascii_lowercase)
        .filter(|digest| is_hex_sha256(digest))
        .ok_or_else(|| InstallError::ChecksumMalformed {
            asset: checksum_name(path),
            detail: "sha256sum produced no valid digest".to_string(),
        })?;
    Ok(digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs_ops::TempDir;
    use crate::release::collect_assets;
    use std::path::PathBuf;

    fn release_with_assets(names: &[&str]) -> serde_json::Value {
        let assets = names
            .iter()
            .map(|name| {
                serde_json::json!({
                    "name": name,
                    "browser_download_url": format!("https://example.test/{name}")
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({ "assets": assets })
    }

    fn write_temp(name: &str, contents: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new("checksum-test").expect("temp dir");
        let path = dir.path().join(name);
        fs::write(&path, contents).expect("write checksum file");
        (dir, path)
    }

    /// Build a `Cli` with verification defaults (everything off). Tests flip
    /// individual flags to exercise the verification policy.
    fn verify_cli() -> Cli {
        Cli {
            dry_run: false,
            force: false,
            no_verify: false,
            allow_unverified: false,
        }
    }

    #[test]
    fn missing_checksum_fails_closed_by_default() {
        // A release that ships no checksum at all.
        let release = release_with_assets(&["rexops-x86_64-unknown-linux-gnu.tar.gz"]);
        let assets = collect_assets(&release);
        let asset = ReleaseAsset {
            name: "rexops-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            download_url: String::new(),
        };
        let (dir, downloaded) = write_temp(&asset.name, "payload");

        let err = verify_download(&verify_cli(), &assets, &asset, &downloaded, dir.path())
            .expect_err("missing checksum must fail closed by default");
        assert!(matches!(err, InstallError::ChecksumMissing { .. }));
    }

    #[test]
    fn missing_checksum_allowed_with_flag() {
        let release = release_with_assets(&["rexops-x86_64-unknown-linux-gnu.tar.gz"]);
        let assets = collect_assets(&release);
        let asset = ReleaseAsset {
            name: "rexops-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            download_url: String::new(),
        };
        let (dir, downloaded) = write_temp(&asset.name, "payload");

        let cli = Cli {
            allow_unverified: true,
            ..verify_cli()
        };
        verify_download(&cli, &assets, &asset, &downloaded, dir.path())
            .expect("--allow-unverified must permit a missing checksum");
    }

    #[test]
    fn no_verify_skips_missing_checksum() {
        let release = release_with_assets(&["rexops-x86_64-unknown-linux-gnu.tar.gz"]);
        let assets = collect_assets(&release);
        let asset = ReleaseAsset {
            name: "rexops-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            download_url: String::new(),
        };
        let (dir, downloaded) = write_temp(&asset.name, "payload");

        let cli = Cli {
            no_verify: true,
            ..verify_cli()
        };
        verify_download(&cli, &assets, &asset, &downloaded, dir.path())
            .expect("--no-verify must skip verification entirely");
    }

    const DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn reads_bare_hex_digest() {
        let (_dir, path) = write_temp("file.sha256", DIGEST);
        let got = read_expected_sha256(&path, "anything.tar.gz").expect("digest");
        assert_eq!(got, DIGEST);
    }

    #[test]
    fn reads_sha256sum_line_format() {
        let (_dir, path) = write_temp("file.sha256", &format!("{DIGEST}  rexops-linux.tar.gz\n"));
        let got = read_expected_sha256(&path, "rexops-linux.tar.gz").expect("digest");
        assert_eq!(got, DIGEST);
    }

    #[test]
    fn reads_matching_line_from_multi_entry_manifest() {
        let other = "1111111111111111111111111111111111111111111111111111111111111111";
        // Second line uses sha256sum binary-mode format: `<hex> *<filename>`.
        let manifest = format!(
            "{other}  bulwark-linux.tar.gz\n{DIGEST} *rexops-linux.tar.gz\n",
            other = other,
            DIGEST = DIGEST
        );
        let (_dir, path) = write_temp("SHA256SUMS", &manifest);
        let got = read_expected_sha256(&path, "rexops-linux.tar.gz").expect("digest");
        assert_eq!(got, DIGEST);
    }

    #[test]
    fn malformed_checksum_file_errors() {
        let (_dir, path) = write_temp("file.sha256", "not-a-valid-digest\n");
        let err = read_expected_sha256(&path, "rexops-linux.tar.gz").unwrap_err();
        assert!(matches!(err, InstallError::ChecksumMalformed { .. }));
    }

    #[test]
    fn manifest_without_matching_filename_errors() {
        let manifest = format!("{DIGEST}  some-other-file.tar.gz\n");
        let (_dir, path) = write_temp("SHA256SUMS", &manifest);
        let err = read_expected_sha256(&path, "rexops-linux.tar.gz").unwrap_err();
        assert!(matches!(err, InstallError::ChecksumMalformed { .. }));
    }

    #[test]
    fn bare_digest_rejected_when_manifest_has_multiple_entries() {
        // A bare (filename-less) digest is ambiguous in a multi-entry manifest:
        // we must NOT guess that it belongs to the requested archive.
        let other = "1111111111111111111111111111111111111111111111111111111111111111";
        let manifest = format!("{other}  bulwark-linux.tar.gz\n{DIGEST}\n");
        let (_dir, path) = write_temp("SHA256SUMS", &manifest);
        let err = read_expected_sha256(&path, "rexops-linux.tar.gz").unwrap_err();
        assert!(matches!(err, InstallError::ChecksumMalformed { .. }));
    }

    #[test]
    fn bare_digest_accepted_only_when_sole_entry() {
        // Exactly one digest line, no filename: unambiguous, so trust it.
        let (_dir, path) = write_temp("file.sha256", &format!("{DIGEST}\n"));
        let got = read_expected_sha256(&path, "rexops-linux.tar.gz").expect("digest");
        assert_eq!(got, DIGEST);
    }

    #[test]
    fn sha256_of_file_matches_known_empty_digest() {
        let (_dir, path) = write_temp("empty", "");
        let got = sha256_of_file(&path).expect("digest");
        assert_eq!(got, DIGEST);
    }

    #[test]
    fn is_hex_sha256_rejects_wrong_length_and_nonhex() {
        assert!(is_hex_sha256(DIGEST));
        assert!(!is_hex_sha256("abc"));
        assert!(!is_hex_sha256(&"z".repeat(64)));
    }
}
