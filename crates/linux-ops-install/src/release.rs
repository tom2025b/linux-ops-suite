//! GitHub release discovery: the tool registry, the asset model, and the logic
//! that picks the right binary archive (and pairs it with a checksum file) for
//! the current platform.

use serde_json::Value;

use crate::error::InstallError;
use crate::net::fetch_http;
use crate::platform::Platform;
use crate::ui::{detail, warn};
use crate::GITHUB_OWNER;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Tool {
    pub(crate) repo: &'static str,
    pub(crate) binary: &'static str,
}

pub(crate) const TOOLS: &[Tool] = &[
    Tool {
        repo: "bulwark",
        binary: "bulwark",
    },
    Tool {
        repo: "scriptvault",
        binary: "scriptvault",
    },
    Tool {
        repo: "toolfoundry",
        binary: "toolfoundry",
    },
    Tool {
        repo: "workstate",
        binary: "workstate",
    },
    Tool {
        repo: "rexops",
        binary: "rexops",
    },
    // In-workspace tools: built from this umbrella repo and shipped together in
    // one `linux-ops-suite-<target>` release archive (matched by repo name;
    // `find_binary` then extracts each by its own binary name). Keep this list in
    // sync with release.yml's package step and the README "Supported binaries"
    // list. rex-check is intentionally omitted — it is a dev/health tool, not part
    // of the user-facing tool set in the README.
    Tool {
        repo: "linux-ops-suite",
        binary: "toolbox-bridge",
    },
    Tool {
        repo: "linux-ops-suite",
        binary: "rex-doctor",
    },
    Tool {
        repo: "linux-ops-suite",
        binary: "portman",
    },
    Tool {
        repo: "linux-ops-suite",
        binary: "pulse",
    },
    Tool {
        repo: "linux-ops-suite",
        binary: "tripwire",
    },
    Tool {
        repo: "linux-ops-suite",
        binary: "rewind",
    },
    Tool {
        repo: "linux-ops-suite",
        binary: "conductor",
    },
    Tool {
        repo: "linux-ops-suite",
        binary: "rex-forge",
    },
    // Consolidated in-tree (was a standalone repo): producer of the proto feed.
    Tool {
        repo: "linux-ops-suite",
        binary: "proto",
    },
];

#[derive(Clone, Debug)]
pub(crate) struct ReleaseAsset {
    pub(crate) name: String,
    pub(crate) download_url: String,
}

/// All assets in a release, indexed by name, so we can pair a binary archive
/// with its sibling `.sha256` checksum file for integrity verification.
#[derive(Debug)]
pub(crate) struct ReleaseAssets {
    pub(crate) all: Vec<ReleaseAsset>,
}

impl ReleaseAssets {
    /// Locate the checksum asset that verifies `archive`. Producers publish
    /// either `<archive>.sha256` (the common convention) or a single
    /// `SHA256SUMS`/`SHA256SUMS.txt` manifest covering every asset.
    ///
    /// The per-archive sidecar is preferred. The manifest fallback matches only
    /// the exact names `SHA256SUMS`/`SHA256SUMS.txt` (case-insensitively) — not
    /// any `*sha256sums` suffix — so a per-asset file belonging to a *different*
    /// archive can never be mistaken for this archive's manifest.
    pub(crate) fn checksum_for(&self, archive: &str) -> Option<&ReleaseAsset> {
        let sidecar = format!("{archive}.sha256");
        self.all
            .iter()
            .find(|asset| asset.name.eq_ignore_ascii_case(&sidecar))
            .or_else(|| {
                self.all.iter().find(|asset| {
                    let lower = asset.name.to_ascii_lowercase();
                    lower == "sha256sums" || lower == "sha256sums.txt"
                })
            })
    }
}

pub(crate) fn fetch_latest_release(tool: &Tool) -> Result<Value, InstallError> {
    let url = latest_release_api_url(tool.repo);
    let (status, body) = fetch_http(&url)?;

    match status {
        200 => serde_json::from_slice(&body).map_err(|source| InstallError::Json {
            context: format!("parse latest release JSON for {}", tool.repo),
            source,
        }),
        404 => Err(InstallError::NoLatestRelease {
            repo: tool.repo,
            binary: tool.binary,
            releases_url: releases_url(tool.repo),
            new_release_url: new_release_url(tool.repo),
        }),
        _ => Err(InstallError::HttpStatus {
            url,
            status,
            body: summarize_http_body(&body),
        }),
    }
}

/// Collect every named, downloadable asset from a release. We keep checksum and
/// signature files here (unlike binary selection) so `checksum_for` can pair an
/// archive with its `.sha256` sibling.
pub(crate) fn collect_assets(release: &Value) -> ReleaseAssets {
    let assets = release
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let all = assets
        .into_iter()
        .filter_map(|asset| {
            let name = asset.get("name").and_then(Value::as_str)?;
            let download_url = asset.get("browser_download_url").and_then(Value::as_str)?;
            // The asset name comes from untrusted GitHub release JSON and is
            // later used as a path component (temp_dir.join(name)) and matched
            // against checksum manifests. Drop any name that isn't a plain
            // filename so it can never traverse out of the temp dir or alias a
            // different asset; the URL must be https so curl --proto=https can
            // fetch it.
            if !is_safe_asset_name(name) || !download_url.starts_with("https://") {
                return None;
            }
            Some(ReleaseAsset {
                name: name.to_string(),
                download_url: download_url.to_string(),
            })
        })
        .collect();

    ReleaseAssets { all }
}

/// True if `name` is a plain, single-segment filename safe to use as a path
/// component: non-empty, no `/` or `\`, not `.`/`..`, and not a hidden/dotfile
/// (a leading dot would let a crafted asset masquerade as a config file). This
/// is the gate that keeps a malicious GitHub asset name from escaping the temp
/// directory via `Path::join`.
fn is_safe_asset_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

pub(crate) fn select_asset(
    tool: &Tool,
    platform: &Platform,
    assets: &ReleaseAssets,
) -> Result<ReleaseAsset, InstallError> {
    let available_names: Vec<String> = assets.all.iter().map(|asset| asset.name.clone()).collect();

    assets
        .all
        .iter()
        .filter(|asset| asset_matches(&asset.name, tool, platform))
        .max_by_key(|asset| asset_score(&asset.name, tool, platform))
        .cloned()
        .ok_or_else(|| InstallError::NoReleaseAsset {
            repo: tool.repo,
            platform: platform.asset_hint(),
            assets: available_names,
        })
}

/// Extensions for detached signatures and checksum sidecars. These are never
/// installable binaries, so binary selection skips them. Kept in one place so
/// the denylist can't drift between callers.
const SIGNATURE_OR_CHECKSUM_EXTENSIONS: &[&str] = &[
    ".sha256",
    ".sha256sum",
    ".sha512",
    ".sha512sum",
    ".asc",
    ".minisig",
    ".sig",
];

/// True if `name` is a detached signature or checksum file rather than an
/// installable binary archive.
fn is_signature_or_checksum(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SIGNATURE_OR_CHECKSUM_EXTENSIONS
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn asset_matches(name: &str, tool: &Tool, platform: &Platform) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains(platform.os)
        && platform
            .arch_aliases
            .iter()
            .any(|alias| lower.contains(alias))
        && (lower.contains(&tool.binary.to_ascii_lowercase())
            || lower.contains(&tool.repo.to_ascii_lowercase()))
        && !is_signature_or_checksum(name)
}

fn asset_score(name: &str, tool: &Tool, platform: &Platform) -> usize {
    let lower = name.to_ascii_lowercase();
    let mut score = 0;
    if lower.contains(&tool.binary.to_ascii_lowercase()) {
        score += 100;
    }
    if lower.contains(&tool.repo.to_ascii_lowercase()) {
        score += 50;
    }
    if lower.contains(platform.os) {
        score += 20;
    }
    if lower.contains(platform.arch) {
        score += 20;
    }
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        score += 12;
    }
    if lower.ends_with(".tar.xz") {
        score += 10;
    }
    if lower.ends_with(".zip") {
        score += 6;
    }
    if !lower.contains("debug") {
        score += 2;
    }
    score
}

pub(crate) fn report_install_error(tool: &Tool, platform: &Platform, err: &InstallError) {
    match err {
        InstallError::NoLatestRelease {
            binary,
            releases_url,
            new_release_url,
            ..
        } => {
            warn(format!("{binary}: no GitHub Release is published yet"));
            detail(format!("releases page : {releases_url}"));
            detail(format!("create release: {new_release_url}"));
            detail(expected_asset_note(platform, binary));
            detail("fallback      : use ./install.sh to build from source right now");
        }
        InstallError::NoReleaseAsset { assets, .. } => {
            warn(format!("{}: {}", tool.binary, err.summary_message()));
            if !assets.is_empty() {
                detail(format!("available assets: {}", assets.join(", ")));
            }
            detail(expected_asset_note(platform, tool.binary));
        }
        _ => warn(format!("{}: {}", tool.binary, err.summary_message())),
    }
}

pub(crate) fn latest_release_api_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{GITHUB_OWNER}/{repo}/releases/latest")
}

pub(crate) fn releases_url(repo: &str) -> String {
    format!("https://github.com/{GITHUB_OWNER}/{repo}/releases")
}

pub(crate) fn new_release_url(repo: &str) -> String {
    format!("https://github.com/{GITHUB_OWNER}/{repo}/releases/new")
}

fn summarize_http_body(body: &[u8]) -> String {
    const MAX: usize = 240;
    let text = String::from_utf8_lossy(body).replace('\n', " ");
    let trimmed = text.trim();
    if trimmed.len() <= MAX {
        trimmed.to_string()
    } else {
        // Truncate on a UTF-8 char boundary at or below MAX so a multi-byte
        // character straddling the limit can never trigger a slice panic.
        let end = trimmed
            .char_indices()
            .map(|(idx, _)| idx)
            .take_while(|&idx| idx <= MAX)
            .last()
            .unwrap_or(0);
        format!("{}...", &trimmed[..end])
    }
}

fn expected_asset_note(platform: &Platform, binary: &str) -> String {
    format!(
        "expected asset : a Linux archive for `{}` matching `{}`; prefer .tar.gz",
        binary,
        platform.asset_hint()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux_x86_64() -> Platform {
        Platform {
            os: "linux",
            arch: "x86_64",
            arch_aliases: &["x86_64", "amd64"],
        }
    }

    fn linux_aarch64() -> Platform {
        Platform {
            os: "linux",
            arch: "aarch64",
            arch_aliases: &["aarch64", "arm64"],
        }
    }

    fn release_with_assets(names: &[&str]) -> Value {
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

    #[test]
    fn is_safe_asset_name_accepts_plain_filenames() {
        assert!(is_safe_asset_name(
            "bulwark-x86_64-unknown-linux-gnu.tar.gz"
        ));
        assert!(is_safe_asset_name("rexops.sha256"));
    }

    #[test]
    fn is_safe_asset_name_rejects_traversal_and_tricks() {
        assert!(!is_safe_asset_name(""));
        assert!(!is_safe_asset_name("."));
        assert!(!is_safe_asset_name(".."));
        assert!(!is_safe_asset_name("../../etc/passwd"));
        assert!(!is_safe_asset_name("/etc/cron.d/evil"));
        assert!(!is_safe_asset_name("sub/dir/file.tar.gz"));
        assert!(!is_safe_asset_name("a\\b"));
        assert!(!is_safe_asset_name(".bashrc"));
        assert!(!is_safe_asset_name("nul\0byte"));
    }

    #[test]
    fn collect_assets_drops_unsafe_names_and_non_https() {
        // A traversal name, a non-https URL, and a good asset; only the good one
        // should survive collection.
        let release = serde_json::json!({
            "assets": [
                { "name": "../escape.tar.gz", "browser_download_url": "https://example.test/x" },
                { "name": "good.tar.gz", "browser_download_url": "http://example.test/good.tar.gz" },
                { "name": "good.tar.gz", "browser_download_url": "https://example.test/good.tar.gz" },
            ]
        });
        let assets = collect_assets(&release);
        assert_eq!(assets.all.len(), 1);
        assert_eq!(assets.all[0].name, "good.tar.gz");
        assert!(assets.all[0].download_url.starts_with("https://"));
    }

    #[test]
    fn selects_matching_linux_archive_for_binary() {
        let tool = Tool {
            repo: "bulwark",
            binary: "bulwark",
        };
        let release = release_with_assets(&[
            "bulwark-aarch64-unknown-linux-gnu.tar.gz",
            "bulwark-x86_64-unknown-linux-gnu.tar.gz",
            "bulwark-x86_64-apple-darwin.tar.gz",
        ]);

        let asset = select_asset(&tool, &linux_x86_64(), &collect_assets(&release))
            .expect("matching asset");

        assert_eq!(asset.name, "bulwark-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn selects_matching_aarch64_archive() {
        let tool = Tool {
            repo: "workstate",
            binary: "workstate",
        };
        let release = release_with_assets(&[
            "workstate-x86_64-unknown-linux-gnu.tar.gz",
            "workstate-aarch64-unknown-linux-gnu.tar.gz",
        ]);

        let asset = select_asset(&tool, &linux_aarch64(), &collect_assets(&release))
            .expect("matching asset");

        assert_eq!(asset.name, "workstate-aarch64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn prefers_tar_gz_over_other_archive_formats() {
        let tool = Tool {
            repo: "scriptvault",
            binary: "scriptvault",
        };
        let release = release_with_assets(&[
            "scriptvault-x86_64-unknown-linux-gnu.zip",
            "scriptvault-x86_64-unknown-linux-gnu.tar.xz",
            "scriptvault-x86_64-unknown-linux-gnu.tar.gz",
        ]);

        let asset = select_asset(&tool, &linux_x86_64(), &collect_assets(&release))
            .expect("matching asset");

        assert_eq!(asset.name, "scriptvault-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn does_not_select_checksum_or_signature_as_binary() {
        let tool = Tool {
            repo: "proto",
            binary: "proto",
        };
        let release = release_with_assets(&[
            "proto-x86_64-unknown-linux-gnu.tar.gz.sha256",
            "proto-x86_64-unknown-linux-gnu.tar.gz.sig",
            "proto-x86_64-unknown-linux-gnu.tar.gz.asc",
            "proto-x86_64-unknown-linux-gnu.tar.gz",
        ]);

        // The binary selector still skips checksum/sig files...
        let asset = select_asset(&tool, &linux_x86_64(), &collect_assets(&release))
            .expect("matching asset");
        assert_eq!(asset.name, "proto-x86_64-unknown-linux-gnu.tar.gz");

        // ...but the checksum sibling is now reachable for verification.
        let checksum = collect_assets(&release)
            .checksum_for("proto-x86_64-unknown-linux-gnu.tar.gz")
            .map(|asset| asset.name.clone());
        assert_eq!(
            checksum.as_deref(),
            Some("proto-x86_64-unknown-linux-gnu.tar.gz.sha256")
        );
    }

    #[test]
    fn checksum_for_finds_sha256sums_manifest_when_no_sidecar() {
        let release =
            release_with_assets(&["rexops-x86_64-unknown-linux-gnu.tar.gz", "SHA256SUMS"]);
        let checksum = collect_assets(&release)
            .checksum_for("rexops-x86_64-unknown-linux-gnu.tar.gz")
            .map(|asset| asset.name.clone());
        assert_eq!(checksum.as_deref(), Some("SHA256SUMS"));
    }

    #[test]
    fn checksum_for_prefers_sidecar_over_manifest() {
        let release = release_with_assets(&[
            "rexops-x86_64-unknown-linux-gnu.tar.gz",
            "rexops-x86_64-unknown-linux-gnu.tar.gz.sha256",
            "SHA256SUMS",
        ]);
        let checksum = collect_assets(&release)
            .checksum_for("rexops-x86_64-unknown-linux-gnu.tar.gz")
            .map(|asset| asset.name.clone());
        assert_eq!(
            checksum.as_deref(),
            Some("rexops-x86_64-unknown-linux-gnu.tar.gz.sha256")
        );
    }

    #[test]
    fn checksum_for_returns_none_when_absent() {
        let release = release_with_assets(&["rexops-x86_64-unknown-linux-gnu.tar.gz"]);
        assert!(collect_assets(&release)
            .checksum_for("rexops-x86_64-unknown-linux-gnu.tar.gz")
            .is_none());
    }

    #[test]
    fn checksum_for_ignores_loosely_named_manifest() {
        // A per-asset file ending in `sha256sums` that belongs to a DIFFERENT
        // archive must not be paired as this archive's manifest.
        let release = release_with_assets(&[
            "rexops-x86_64-unknown-linux-gnu.tar.gz",
            "bulwark.tar.gz.sha256sums",
        ]);
        assert!(collect_assets(&release)
            .checksum_for("rexops-x86_64-unknown-linux-gnu.tar.gz")
            .is_none());
    }

    #[test]
    fn accepts_repo_named_umbrella_archive_for_workspace_tool() {
        let tool = Tool {
            repo: "linux-ops-suite",
            binary: "toolbox-bridge",
        };
        let release = release_with_assets(&[
            "linux-ops-suite-aarch64-unknown-linux-gnu.tar.xz",
            "linux-ops-suite-x86_64-unknown-linux-gnu.tar.xz",
        ]);

        let asset = select_asset(&tool, &linux_x86_64(), &collect_assets(&release))
            .expect("matching asset");

        assert_eq!(
            asset.name,
            "linux-ops-suite-x86_64-unknown-linux-gnu.tar.xz"
        );
    }

    #[test]
    fn summarize_http_body_truncates_on_char_boundary_without_panicking() {
        // 'é' is two bytes; a run of them straddles the 240-byte boundary.
        // Naive byte slicing would panic mid-character — this must not.
        let body = "é".repeat(200);
        let summary = summarize_http_body(body.as_bytes());
        assert!(summary.ends_with("..."));
        // Result is valid UTF-8 by construction (no panic) and stays within cap.
        assert!(summary.len() <= 240 + "...".len());
    }

    #[test]
    fn missing_release_summary_points_to_releases_page() {
        let error = InstallError::NoLatestRelease {
            repo: "bulwark",
            binary: "bulwark",
            releases_url: releases_url("bulwark"),
            new_release_url: new_release_url("bulwark"),
        };

        assert_eq!(
            error.summary_message(),
            "no GitHub Release is published yet; see https://github.com/tom2025b/bulwark/releases"
        );
    }
}
