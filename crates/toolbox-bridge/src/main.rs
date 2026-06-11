//! Toolbox-Bridge CLI: read Bulwark findings from the Workstate snapshot,
//! convert them to ScriptVault sidecar records, publish them as a Workstate
//! feed. Pure adapter — it never talks to Bulwark or ScriptVault directly.

use std::path::PathBuf;
use std::process::ExitCode;

use chrono::Utc;
use clap::Parser;

use toolbox_bridge::error::BridgeError;
use toolbox_bridge::snapshot::status_label;
use toolbox_bridge::{convert, feed, snapshot};

/// Bridge Bulwark findings into a ScriptVault sidecar feed, via Workstate.
///
/// Reads the compiled Workstate snapshot (never Bulwark directly), converts
/// its findings section into ScriptVault sidecar metadata (risk/owner tags
/// plus a badged description), and writes the result as a versioned feed
/// into Workstate's feeds directory for ScriptVault to consume.
#[derive(Parser)]
#[command(name = "toolbox-bridge", version, about, verbatim_doc_comment)]
struct Cli {
    /// Path to the Workstate snapshot to read.
    ///
    /// [default: $XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json,
    /// fallback ~/.local/share/...]
    #[arg(long, value_name = "PATH")]
    snapshot: Option<PathBuf>,

    /// Where to write the sidecar feed.
    ///
    /// [default: $XDG_DATA_HOME/workstate/feeds/toolbox-bridge.json,
    /// fallback ~/.local/share/...]
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,

    /// Print the feed to stdout instead of writing it (preview; changes
    /// nothing on disk).
    #[arg(long)]
    dry_run: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("toolbox-bridge: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), BridgeError> {
    // -- Resolve paths --------------------------------------------------------
    let snapshot_path = match cli.snapshot {
        Some(path) => path,
        None => snapshot::default_snapshot_path()?,
    };
    let output_path = match cli.output {
        Some(path) => path,
        None => feed::default_feed_path()?,
    };

    // -- Read (from Workstate only) -------------------------------------------
    let snap = snapshot::load_snapshot(&snapshot_path)?;
    let view = snapshot::findings_view(&snap)?;
    if view.stale {
        eprintln!(
            "toolbox-bridge: warning: findings section is Stale (Bulwark feed from {}); converting anyway",
            view.inventory.generated_at
        );
    }

    // -- Convert (pure) -------------------------------------------------------
    let conversion = convert::convert(&view.inventory.findings);
    for skip in &conversion.skipped {
        eprintln!(
            "toolbox-bridge: skipping '{}': {}",
            skip.subject, skip.reason
        );
    }

    let sidecar_count = conversion.sidecars.len();
    let skip_count = conversion.skipped.len();
    let bridge_feed = feed::SidecarFeed::new(
        conversion.sidecars,
        &view.inventory.generated_at,
        Utc::now(),
    );

    // -- Publish --------------------------------------------------------------
    if cli.dry_run {
        // Preview goes to STDOUT (pipeable); status stays on stderr.
        let json =
            serde_json::to_string_pretty(&bridge_feed).map_err(|e| BridgeError::FeedWrite {
                path: "<stdout>".to_string(),
                source: std::io::Error::other(e),
            })?;
        println!("{json}");
        eprintln!(
            "DRY RUN: would write {sidecar_count} sidecar record(s) ({skip_count} skipped) -> {}",
            output_path.display()
        );
        return Ok(());
    }

    feed::write_feed(&bridge_feed, &output_path)?;

    // One scannable summary line per pipeline stage, like Workstate's own
    // output: proof of what was read, what was produced, and where it went.
    println!("Toolbox-Bridge — Bulwark → ScriptVault sidecars, via Workstate");
    println!(
        "Read snapshot  <- {} (findings: {}, {} finding(s))",
        snapshot_path.display(),
        status_label(&snap.findings.status),
        view.inventory.findings.len()
    );
    println!(
        "Wrote feed     -> {} ({sidecar_count} sidecar record(s), {skip_count} skipped)",
        output_path.display()
    );
    Ok(())
}
