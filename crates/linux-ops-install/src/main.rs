use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use serde_json::Value;

const GITHUB_OWNER: &str = "tom2025b";
const REX_LAUNCHER: &str = include_str!("../../../bin/rex");

const TOOLS: &[Tool] = &[
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
        repo: "proto",
        binary: "proto",
    },
    Tool {
        repo: "rexops",
        binary: "rexops",
    },
    Tool {
        repo: "linux-ops-suite",
        binary: "toolbox-bridge",
    },
];

/// Install and prepare Linux Ops Suite components.
#[derive(Debug, Parser)]
#[command(name = "linux-ops-install", version, about, verbatim_doc_comment)]
struct Cli {
    /// Show what would happen without changing files or system state.
    #[arg(long)]
    dry_run: bool,

    /// Reapply installer steps even when existing files or links are detected.
    #[arg(long)]
    force: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = io::stdout().flush();
            eprintln!("linux-ops-install: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), InstallError> {
    print_banner();
    print_mode(cli);

    let paths = InstallPaths::from_env()?;
    let platform = Platform::current()?;

    println!("Install directory : {}", paths.bin_dir.display());
    println!("Wrapper directory : {}", paths.wrapper_dir.display());
    println!("Aliases file      : {}", paths.aliases_file.display());
    println!("Release target    : {}", platform.asset_hint());
    println!();

    check_prereqs(cli)?;

    let mut failures = Vec::new();
    for tool in TOOLS {
        if let Err(err) = install_tool(cli, &paths, &platform, tool) {
            report_install_error(tool, &platform, &err);
            failures.push(FailureSummary {
                tool: tool.binary,
                message: err.summary_message(),
                missing_release: matches!(err, InstallError::NoLatestRelease { .. }),
            });
        }
    }

    install_rex_launcher(cli, &paths)?;
    install_wrappers_and_aliases(cli, &paths)?;
    print_path_guidance(&paths);

    if !failures.is_empty() {
        return Err(InstallError::PartialFailure(failures));
    }

    Ok(())
}

fn print_banner() {
    println!();
    println!("==================================================");
    println!(" Linux Ops Suite Installer");
    println!("==================================================");
}

fn print_mode(cli: &Cli) {
    if cli.dry_run {
        println!("Mode : dry run");
    } else {
        println!("Mode : install");
    }

    if cli.force {
        println!("Force: enabled");
    } else {
        println!("Force: disabled");
    }
}

#[derive(Clone, Copy, Debug)]
struct Tool {
    repo: &'static str,
    binary: &'static str,
}

#[derive(Debug)]
struct InstallPaths {
    bin_dir: PathBuf,
    wrapper_dir: PathBuf,
    aliases_file: PathBuf,
}

impl InstallPaths {
    fn from_env() -> Result<Self, InstallError> {
        let home = home_dir()?;
        Ok(Self {
            bin_dir: path_from_env("BIN_DIR", &home.join(".local/bin")),
            wrapper_dir: path_from_env("WRAPPER_DIR", &home.join("bin")),
            aliases_file: path_from_env("ALIASES_FILE", &home.join(".rust_aliases.sh")),
        })
    }
}

#[derive(Debug)]
struct Platform {
    os: &'static str,
    arch: &'static str,
    arch_aliases: &'static [&'static str],
}

impl Platform {
    fn current() -> Result<Self, InstallError> {
        if env::consts::OS != "linux" {
            return Err(InstallError::UnsupportedPlatform(format!(
                "{}-{}",
                env::consts::OS,
                env::consts::ARCH
            )));
        }

        let (arch, arch_aliases) = match env::consts::ARCH {
            "x86_64" => ("x86_64", &["x86_64", "amd64"][..]),
            "aarch64" => ("aarch64", &["aarch64", "arm64"][..]),
            other => return Err(InstallError::UnsupportedPlatform(format!("linux-{other}"))),
        };

        Ok(Self {
            os: "linux",
            arch,
            arch_aliases,
        })
    }

    fn asset_hint(&self) -> String {
        format!("{}-{}", self.os, self.arch)
    }
}

#[derive(Debug)]
struct ReleaseAsset {
    name: String,
    download_url: String,
}

#[derive(Debug)]
struct FailureSummary {
    tool: &'static str,
    message: String,
    missing_release: bool,
}

#[derive(Debug)]
enum InstallError {
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
            Self::NoLatestRelease {
                repo, releases_url, ..
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
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn install_tool(
    cli: &Cli,
    paths: &InstallPaths,
    platform: &Platform,
    tool: &Tool,
) -> Result<(), InstallError> {
    let destination = paths.bin_dir.join(tool.binary);
    if !cli.force && is_executable(&destination) {
        skip(format!(
            "{} already installed at {}; use --force to reinstall",
            tool.binary,
            destination.display()
        ));
        return Ok(());
    }

    step(format!(
        "Installing {} from {}/{} latest release",
        tool.binary, GITHUB_OWNER, tool.repo
    ));

    if cli.dry_run {
        dry_run(format!(
            "query https://api.github.com/repos/{}/{}/releases/latest",
            GITHUB_OWNER, tool.repo
        ));
        dry_run(format!(
            "select asset matching {} and binary {}",
            platform.asset_hint(),
            tool.binary
        ));
        dry_run(format!("download and install to {}", destination.display()));
        return Ok(());
    }

    let release = fetch_latest_release(tool)?;
    let asset = select_asset(tool, platform, &release)?;
    let temp_dir = TempDir::new(tool.binary)?;
    let downloaded = temp_dir.path().join(&asset.name);

    download_asset(&asset, &downloaded)?;
    let binary = prepare_binary(&downloaded, temp_dir.path(), tool.binary)?;
    install_binary(&binary, &destination)?;
    ok(format!("{} -> {}", tool.binary, destination.display()));

    Ok(())
}

fn install_rex_launcher(cli: &Cli, paths: &InstallPaths) -> Result<(), InstallError> {
    let destination = paths.bin_dir.join("rex");
    step("Installing rex launcher");

    if cli.dry_run {
        dry_run(format!(
            "write embedded launcher to {}",
            destination.display()
        ));
        return Ok(());
    }

    create_dir_all(&paths.bin_dir, "create install directory")?;
    fs::write(&destination, REX_LAUNCHER).map_err(|source| InstallError::Io {
        context: format!("write {}", destination.display()),
        source,
    })?;
    set_executable(&destination)?;
    ok(format!("rex -> {}", destination.display()));
    Ok(())
}

fn install_wrappers_and_aliases(cli: &Cli, paths: &InstallPaths) -> Result<(), InstallError> {
    step("Installing r-<tool> wrappers and aliases");

    if cli.dry_run {
        dry_run(format!("create {}", paths.wrapper_dir.display()));
        dry_run(format!("create/update {}", paths.aliases_file.display()));
        for tool in TOOLS {
            dry_run(format!(
                "write {}/r-{} and alias r-{}",
                paths.wrapper_dir.display(),
                tool.binary,
                tool.binary
            ));
        }
        return Ok(());
    }

    create_dir_all(&paths.wrapper_dir, "create wrapper directory")?;
    ensure_aliases_file(&paths.aliases_file)?;

    let mut aliases =
        fs::read_to_string(&paths.aliases_file).map_err(|source| InstallError::Io {
            context: format!("read {}", paths.aliases_file.display()),
            source,
        })?;

    for tool in TOOLS {
        let wrapper = paths.wrapper_dir.join(format!("r-{}", tool.binary));
        let script = format!(
            "#!/usr/bin/env bash\n# Auto-generated by linux-ops-install - wrapper for {}.\nexec {} \"$@\"\n",
            tool.binary, tool.binary
        );
        fs::write(&wrapper, script).map_err(|source| InstallError::Io {
            context: format!("write {}", wrapper.display()),
            source,
        })?;
        set_executable(&wrapper)?;

        let alias_prefix = format!("alias r-{}=", tool.binary);
        if !aliases.lines().any(|line| line.starts_with(&alias_prefix)) {
            if !aliases.ends_with('\n') {
                aliases.push('\n');
            }
            aliases.push_str(&format!("alias r-{}='{}'\n", tool.binary, tool.binary));
        }

        ok(format!("r-{} wrapper", tool.binary));
    }

    fs::write(&paths.aliases_file, aliases).map_err(|source| InstallError::Io {
        context: format!("write {}", paths.aliases_file.display()),
        source,
    })?;
    ok(format!("aliases -> {}", paths.aliases_file.display()));

    Ok(())
}

fn check_prereqs(cli: &Cli) -> Result<(), InstallError> {
    step("Checking prerequisites");
    check_command("curl", cli)?;
    if cli.dry_run {
        dry_run("tar/unzip are checked only if the selected release asset needs extraction");
    }
    Ok(())
}

fn check_command(program: &str, cli: &Cli) -> Result<(), InstallError> {
    if cli.dry_run {
        dry_run(format!("check command {program}"));
        return Ok(());
    }

    run_command(Command::new(program).arg("--version"))?;
    ok(format!("{program} found"));
    Ok(())
}

fn fetch_latest_release(tool: &Tool) -> Result<Value, InstallError> {
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

fn select_asset(
    tool: &Tool,
    platform: &Platform,
    release: &Value,
) -> Result<ReleaseAsset, InstallError> {
    let assets = release
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut candidates = Vec::new();
    let mut available_names = Vec::new();

    for asset in assets {
        let Some(name) = asset.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(download_url) = asset.get("browser_download_url").and_then(Value::as_str) else {
            continue;
        };

        available_names.push(name.to_string());
        if asset_matches(name, tool, platform) {
            candidates.push(ReleaseAsset {
                name: name.to_string(),
                download_url: download_url.to_string(),
            });
        }
    }

    candidates
        .into_iter()
        .max_by_key(|asset| asset_score(&asset.name, tool, platform))
        .ok_or_else(|| InstallError::NoReleaseAsset {
            repo: tool.repo,
            platform: platform.asset_hint(),
            assets: available_names,
        })
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
        && !lower.ends_with(".sha256")
        && !lower.ends_with(".sha256sum")
        && !lower.ends_with(".sha512")
        && !lower.ends_with(".sha512sum")
        && !lower.ends_with(".asc")
        && !lower.ends_with(".minisig")
        && !lower.ends_with(".sig")
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

fn download_asset(asset: &ReleaseAsset, destination: &Path) -> Result<(), InstallError> {
    step(format!("Downloading {}", asset.name));
    run_command(
        Command::new("curl")
            .arg("-fL")
            .arg("--progress-bar")
            .arg("-H")
            .arg("User-Agent: linux-ops-install")
            .arg("-o")
            .arg(destination)
            .arg(&asset.download_url),
    )?;
    Ok(())
}

fn prepare_binary(
    downloaded: &Path,
    temp_dir: &Path,
    binary_name: &str,
) -> Result<PathBuf, InstallError> {
    let file_name = downloaded
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if file_name.ends_with(".tar.xz") {
        let extract_dir = temp_dir.join("extract");
        create_dir_all(&extract_dir, "create extraction directory")?;
        run_command(
            Command::new("tar")
                .arg("-xJf")
                .arg(downloaded)
                .arg("-C")
                .arg(&extract_dir),
        )?;
        find_binary(&extract_dir, binary_name)
    } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        let extract_dir = temp_dir.join("extract");
        create_dir_all(&extract_dir, "create extraction directory")?;
        run_command(
            Command::new("tar")
                .arg("-xzf")
                .arg(downloaded)
                .arg("-C")
                .arg(&extract_dir),
        )?;
        find_binary(&extract_dir, binary_name)
    } else if file_name.ends_with(".zip") {
        let extract_dir = temp_dir.join("extract");
        create_dir_all(&extract_dir, "create extraction directory")?;
        run_command(
            Command::new("unzip")
                .arg("-q")
                .arg(downloaded)
                .arg("-d")
                .arg(&extract_dir),
        )?;
        find_binary(&extract_dir, binary_name)
    } else {
        Ok(downloaded.to_path_buf())
    }
}

fn install_binary(source: &Path, destination: &Path) -> Result<(), InstallError> {
    create_dir_all(
        destination.parent().unwrap_or_else(|| Path::new(".")),
        "create install directory",
    )?;
    let source_display = source.display().to_string();
    fs::copy(source, destination).map_err(|source| InstallError::Io {
        context: format!("copy {} to {}", source_display, destination.display()),
        source,
    })?;
    set_executable(destination)
}

fn find_binary(root: &Path, binary_name: &str) -> Result<PathBuf, InstallError> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = fs::metadata(&path).map_err(|source| InstallError::Io {
            context: format!("stat {}", path.display()),
            source,
        })?;
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(|source| InstallError::Io {
                context: format!("read {}", path.display()),
                source,
            })? {
                let entry = entry.map_err(|source| InstallError::Io {
                    context: format!("read entry in {}", path.display()),
                    source,
                })?;
                stack.push(entry.path());
            }
        } else if path.file_name() == Some(OsStr::new(binary_name)) {
            return Ok(path);
        }
    }

    Err(InstallError::Io {
        context: format!("find binary {binary_name} under {}", root.display()),
        source: io::Error::new(io::ErrorKind::NotFound, "binary not found in release asset"),
    })
}

fn print_path_guidance(paths: &InstallPaths) {
    println!();
    println!("Done.");
    if !path_contains(&paths.bin_dir) || !path_contains(&paths.wrapper_dir) {
        println!();
        println!("Add this to your shell rc if needed:");
        println!(
            "    export PATH=\"{}:{}:$PATH\"",
            paths.bin_dir.display(),
            paths.wrapper_dir.display()
        );
    }
    println!();
    println!("Source aliases from your shell rc once:");
    println!(
        "    [ -f \"{}\" ] && source \"{}\"",
        paths.aliases_file.display(),
        paths.aliases_file.display()
    );
    println!();
    println!("Then run a full suite refresh:");
    println!("    rex run");
}

fn ensure_aliases_file(path: &Path) -> Result<(), InstallError> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent, "create aliases directory")?;
    }
    if !path.exists() {
        fs::write(path, "# Rust tool aliases - sourced from your shell rc.\n").map_err(
            |source| InstallError::Io {
                context: format!("write {}", path.display()),
                source,
            },
        )?;
    }
    Ok(())
}

fn set_executable(path: &Path) -> Result<(), InstallError> {
    let metadata = fs::metadata(path).map_err(|source| InstallError::Io {
        context: format!("stat {}", path.display()),
        source,
    })?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|source| InstallError::Io {
        context: format!("chmod +x {}", path.display()),
        source,
    })
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn path_contains(path: &Path) -> bool {
    let Ok(path_var) = env::var("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|entry| entry == path)
}

fn path_from_env(name: &str, default: &Path) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| default.to_path_buf())
}

fn home_dir() -> Result<PathBuf, InstallError> {
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or(InstallError::MissingHome)
}

fn create_dir_all(path: &Path, context: &str) -> Result<(), InstallError> {
    fs::create_dir_all(path).map_err(|source| InstallError::Io {
        context: format!("{context}: {}", path.display()),
        source,
    })
}

fn fetch_http(url: &str) -> Result<(u16, Vec<u8>), InstallError> {
    let output = run_command(
        Command::new("curl")
            .arg("-sSL")
            .arg("-H")
            .arg("Accept: application/vnd.github+json")
            .arg("-H")
            .arg("User-Agent: linux-ops-install")
            .arg("-w")
            .arg("\n__HTTP_STATUS__:%{http_code}")
            .arg(url),
    )?;

    let response = String::from_utf8_lossy(&output);
    let Some((body, status)) = response.rsplit_once("\n__HTTP_STATUS__:") else {
        return Err(InstallError::HttpStatus {
            url: url.to_string(),
            status: 0,
            body: "could not parse HTTP status from curl output".to_string(),
        });
    };

    let status = status
        .trim()
        .parse::<u16>()
        .map_err(|source| InstallError::Io {
            context: format!("parse HTTP status for {url}"),
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })?;

    Ok((status, body.as_bytes().to_vec()))
}

fn run_command(command: &mut Command) -> Result<Vec<u8>, InstallError> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command.output().map_err(|source| InstallError::Io {
        context: format!("run {program}"),
        source,
    })?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(InstallError::CommandFailed {
            program,
            status: output
                .status
                .code()
                .map(|code| format!("exit code {code}"))
                .unwrap_or_else(|| "signal".to_string()),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn report_install_error(tool: &Tool, platform: &Platform, err: &InstallError) {
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

fn latest_release_api_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{GITHUB_OWNER}/{repo}/releases/latest")
}

fn releases_url(repo: &str) -> String {
    format!("https://github.com/{GITHUB_OWNER}/{repo}/releases")
}

fn new_release_url(repo: &str) -> String {
    format!("https://github.com/{GITHUB_OWNER}/{repo}/releases/new")
}

fn summarize_http_body(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body).replace('\n', " ");
    let trimmed = text.trim();
    if trimmed.len() <= 240 {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..240])
    }
}

fn expected_asset_note(platform: &Platform, binary: &str) -> String {
    format!(
        "expected asset : a Linux archive for `{}` matching `{}`; prefer .tar.gz",
        binary,
        platform.asset_hint()
    )
}

impl InstallError {
    fn summary_message(&self) -> String {
        match self {
            Self::NoLatestRelease { releases_url, .. } => {
                format!("no GitHub Release is published yet; see {releases_url}")
            }
            _ => self.to_string(),
        }
    }
}

fn step(message: impl AsRef<str>) {
    println!("==> {}", message.as_ref());
}

fn ok(message: impl AsRef<str>) {
    println!("  ok {}", message.as_ref());
}

fn skip(message: impl AsRef<str>) {
    println!("  skip {}", message.as_ref());
}

fn warn(message: impl AsRef<str>) {
    println!("  warn {}", message.as_ref());
}

fn detail(message: impl AsRef<str>) {
    println!("       {}", message.as_ref());
}

fn dry_run(message: impl AsRef<str>) {
    println!("  dry-run {}", message.as_ref());
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Result<Self, InstallError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = env::temp_dir().join(format!(
            "linux-ops-install-{prefix}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).map_err(|source| InstallError::Io {
            context: format!("create temporary directory {}", path.display()),
            source,
        })?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
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

        let asset = select_asset(&tool, &linux_x86_64(), &release).expect("matching asset");

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

        let asset = select_asset(&tool, &linux_aarch64(), &release).expect("matching asset");

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

        let asset = select_asset(&tool, &linux_x86_64(), &release).expect("matching asset");

        assert_eq!(asset.name, "scriptvault-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn ignores_checksum_and_signature_assets() {
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

        let asset = select_asset(&tool, &linux_x86_64(), &release).expect("matching asset");

        assert_eq!(asset.name, "proto-x86_64-unknown-linux-gnu.tar.gz");
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

        let asset = select_asset(&tool, &linux_x86_64(), &release).expect("matching asset");

        assert_eq!(
            asset.name,
            "linux-ops-suite-x86_64-unknown-linux-gnu.tar.xz"
        );
    }

    #[test]
    fn wrapper_alias_append_preserves_existing_line_without_trailing_newline() {
        let temp_dir = TempDir::new("alias-test").expect("temp dir");
        let paths = InstallPaths {
            bin_dir: temp_dir.path().join("local-bin"),
            wrapper_dir: temp_dir.path().join("bin"),
            aliases_file: temp_dir.path().join("aliases.sh"),
        };
        fs::write(&paths.aliases_file, "alias existing='existing'").expect("seed aliases");

        install_wrappers_and_aliases(
            &Cli {
                dry_run: false,
                force: false,
            },
            &paths,
        )
        .expect("install wrappers");

        let aliases = fs::read_to_string(&paths.aliases_file).expect("read aliases");

        assert!(aliases.contains("alias existing='existing'\nalias r-bulwark='bulwark'\n"));
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
