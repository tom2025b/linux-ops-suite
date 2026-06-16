//! The installer's error type: one structured enum with actionable `Display`
//! messages and `Error::source` wiring for the I/O and JSON variants.

use std::io;

use crate::GITHUB_OWNER;

#[derive(Debug)]
pub(crate) struct FailureSummary {
    pub(crate) tool: &'static str,
    pub(crate) message: String,
    pub(crate) missing_release: bool,
}

#[derive(Debug)]
pub(crate) enum InstallError {
    CommandFailed {
        program: String,
        status: String,
        stderr: String,
    },
    HttpStatus {
        url: String,
        status: u16,
        body: String,
    },
    Io {
        context: String,
        source: io::Error,
    },
    Json {
        context: String,
        source: serde_json::Error,
    },
    MissingHome,
    MissingPrerequisite {
        program: String,
        source: io::Error,
    },
    NoLatestRelease {
        repo: &'static str,
        binary: &'static str,
        releases_url: String,
        new_release_url: String,
    },
    NoReleaseAsset {
        repo: &'static str,
        platform: String,
        assets: Vec<String>,
    },
    PartialFailure(Vec<FailureSummary>),
    UnsupportedPlatform(String),
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
    ChecksumMissing {
        asset: String,
    },
    ChecksumMalformed {
        asset: String,
        detail: String,
    },
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandFailed {
                program,
                status,
                stderr,
            } => {
                if stderr.trim().is_empty() {
                    write!(f, "{program} failed with {status}")
                } else {
                    write!(f, "{program} failed with {status}: {}", stderr.trim())
                }
            }
            Self::HttpStatus { url, status, body } => {
                if body.is_empty() {
                    write!(f, "{url} returned HTTP {status}")
                } else {
                    write!(f, "{url} returned HTTP {status}: {body}")
                }
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::Json { context, source } => write!(f, "{context}: {source}"),
            Self::MissingHome => write!(f, "could not determine HOME"),
            Self::MissingPrerequisite { program, .. } => write!(
                f,
                "required tool `{program}` was not found on PATH; install it (e.g. via your distro's package manager) and re-run"
            ),
            // `binary` and `new_release_url` are carried for report_install_error's
            // richer hint, not for this one-line Display — named here (rather than
            // hidden behind `..`) so the omission is obviously deliberate.
            Self::NoLatestRelease {
                repo,
                releases_url,
                binary: _,
                new_release_url: _,
            } => write!(
                f,
                "no GitHub Release is published yet for {GITHUB_OWNER}/{repo}; see {releases_url}"
            ),
            Self::NoReleaseAsset {
                repo,
                platform,
                assets,
            } => {
                write!(f, "no GitHub release asset for {repo} matching {platform}")?;
                if assets.is_empty() {
                    write!(f, " (release has no assets)")
                } else {
                    write!(f, " (available assets: {})", assets.join(", "))
                }
            }
            Self::PartialFailure(failures) => {
                write!(f, "{} tool install(s) failed", failures.len())?;
                let missing_release_count = failures
                    .iter()
                    .filter(|failure| failure.missing_release)
                    .count();
                for failure in failures {
                    write!(f, "\n  - {}: {}", failure.tool, failure.message)?;
                }
                if missing_release_count > 0 {
                    write!(
                        f,
                        "\n\n{} tool(s) are missing GitHub Releases. `linux-ops-install` downloads prebuilt release assets only. Publish Linux x86_64 or aarch64 assets first, preferably `.tar.gz` archives containing the expected binary, or use `./install.sh` to build from source right now.",
                        missing_release_count
                    )?;
                }
                Ok(())
            }
            Self::UnsupportedPlatform(platform) => {
                write!(f, "unsupported platform {platform}; this installer currently expects Linux release binaries")
            }
            Self::ChecksumMismatch {
                asset,
                expected,
                actual,
            } => write!(
                f,
                "SHA256 mismatch for {asset}: expected {expected}, got {actual} (download corrupt or tampered; refusing to install)"
            ),
            Self::ChecksumMissing { asset } => write!(
                f,
                "no SHA256 checksum published for {asset}; refusing to install (pass --allow-unverified to override, or --no-verify to skip verification entirely)"
            ),
            Self::ChecksumMalformed { asset, detail } => {
                write!(f, "could not read SHA256 checksum for {asset}: {detail}")
            }
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::MissingPrerequisite { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl InstallError {
    pub(crate) fn summary_message(&self) -> String {
        match self {
            Self::NoLatestRelease { releases_url, .. } => {
                format!("no GitHub Release is published yet; see {releases_url}")
            }
            _ => self.to_string(),
        }
    }
}
