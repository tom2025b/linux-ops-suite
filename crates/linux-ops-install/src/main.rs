use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

mod error;
mod fs_ops;
mod net;
mod platform;
mod release;
mod ui;
mod verify;

use crate::error::{FailureSummary, InstallError};
use crate::fs_ops::{
    check_prereqs, install_tool, install_wrappers_and_aliases, path_contains, InstallPaths,
};
use crate::platform::Platform;
use crate::release::{report_install_error, TOOLS};
use crate::ui::{print_banner, print_mode, print_path_guidance};

const GITHUB_OWNER: &str = "tom2025b";

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

    /// Skip SHA256 checksum verification entirely. Unsafe: downloads are
    /// installed without integrity checks. Only for local/offline testing.
    #[arg(long)]
    no_verify: bool,

    /// Allow installing a release that publishes no SHA256 checksum, with a
    /// loud warning instead of failing. By default a missing checksum is a hard
    /// failure: every Linux Ops Suite release publishes one, so its absence
    /// means a broken or tampered release. (Checksum *mismatches* always fail,
    /// regardless of this flag.)
    #[arg(long)]
    allow_unverified: bool,
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

    install_wrappers_and_aliases(cli, &paths)?;
    print_path_guidance(&paths);

    if !failures.is_empty() {
        return Err(InstallError::PartialFailure(failures));
    }

    Ok(())
}
