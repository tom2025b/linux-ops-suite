// `anyhow::Result` is the binary's catch-all result type: main() just needs to
// report-and-exit on error, so it doesn't need the typed enums the library uses.
use anyhow::{Context, Result};
use std::path::PathBuf;

// The library's public surface: the builder, the three adapters, the writer, and
// the section/status types we read to print a summary.
use workstate::compile::SnapshotBuilder;
use workstate::ingest::bulwark::BulwarkFeed;
use workstate::ingest::proto::ProtoFeed;
use workstate::ingest::scriptvault::ScriptVaultFeed;
use workstate::ingest::toolfoundry::ToolFoundryFeed;
use workstate::model::provenance::FeedStatus;
use workstate::write_snapshot;

// The canonical output path now lives in `workstate_schema::default_output_path`
// (re-exported as `workstate::default_output_path`) so the producer and every
// consumer agree on ONE location. `main` calls it directly below; `argv[1]` still
// overrides it for tests/CI.

/// Resolve a producer tool's binary. Returns the `WORKSTATE_<KEY>_BIN` override if
/// set and non-empty, else the bare `default` name (resolved on `$PATH` at spawn).
///
/// The override exists for the common dev case where a tool is built but not
/// installed on `$PATH` (e.g. `~/projects/<tool>/target/release/<tool>`): point
/// `WORKSTATE_BULWARK_BIN` at it and Workstate spawns that, with NO sibling-repo
/// layout baked into this binary. An unset/blank override falls back to the name,
/// and a name that isn't installed degrades that section to Missing (not an error).
fn tool_bin(key: &str, default: &str) -> String {
    std::env::var(format!("WORKSTATE_{key}_BIN"))
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Help text shown for `-h`/`--help`. Workstate has no subcommands: it compiles a
/// snapshot and optionally takes a single positional OUTPUT path override.
fn print_help() {
    println!("Workstate - Central State Compiler for Linux Ops Suite");
    println!();
    println!("Compiles a suite-wide snapshot by running each tool's `workstate-feed`");
    println!("subcommand live (bulwark, scriptvault, toolfoundry) and normalizing the");
    println!("output. A tool that isn't installed degrades to a Missing section.");
    println!();
    println!("USAGE:");
    println!("    workstate [OUTPUT]");
    println!();
    println!("ARGS:");
    println!("    [OUTPUT]    Path to write snapshot.json to. Defaults to the shared");
    println!("                RexOps feed path ($XDG_DATA_HOME/rexops/feeds/");
    println!("                workstate.snapshot.json, fallback ~/.local/share/...).");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help       Print this help and exit");
    println!("    -V, --version    Print version and exit");
    println!();
    println!("ENVIRONMENT:");
    println!("    WORKSTATE_BULWARK_BIN, WORKSTATE_SCRIPTVAULT_BIN,");
    println!("    WORKSTATE_TOOLFOUNDRY_BIN   Override a producer's binary path when it");
    println!("                                is not on $PATH (e.g. a built target/release).");
}

fn main() -> Result<()> {
    // -- Handle flags ---------------------------------------------------------
    // Workstate takes at most ONE positional arg (an output-path override). It is
    // NOT a flag parser, but it must not silently treat `--help` as a filename, so
    // any leading-`-` argument is intercepted here: known flags act, unknown flags
    // are rejected with a clear error instead of becoming a stray output file.
    if let Some(arg) = std::env::args().nth(1) {
        if arg.starts_with('-') {
            match arg.as_str() {
                "-h" | "--help" => {
                    print_help();
                    return Ok(());
                }
                "-V" | "--version" => {
                    println!("workstate {}", env!("CARGO_PKG_VERSION"));
                    return Ok(());
                }
                other => {
                    anyhow::bail!(
                        "unknown option '{other}'\n\nWorkstate takes at most one positional OUTPUT path. Run `workstate --help`."
                    );
                }
            }
        }
    }

    // -- Resolve output path --------------------------------------------------
    // `CARGO_MANIFEST_DIR` is the crate root, substituted at COMPILE time, so the
    // fallback path does not depend on where the binary is launched from.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    // OUTPUT: argv[1] if given, else the shared standard path RexOps reads
    // (`$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json`) so `workstate` then
    // `rexops` Just Works with no piping. Only if neither $XDG_DATA_HOME nor $HOME
    // is set do we fall back to an in-crate `<crate>/out/snapshot.json`. The writer
    // creates any missing parent directory, so no manual mkdir is needed.
    let output_path: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(workstate::default_output_path)
        .unwrap_or_else(|| PathBuf::from(format!("{manifest_dir}/out/snapshot.json")));

    // -- Build the adapters (LIVE: spawn the real producer tools) -------------
    // INPUT is no longer a committed fixture. Each feed runs its tool's
    // `workstate-feed` subcommand as a subprocess and ingests its stdout, so the
    // snapshot reflects current state — and freshness is real: a tool just observed
    // its data, so `generated_at` is now and the section reads Fresh. Re-running
    // `workstate` actually refreshes, which is the whole point.
    //
    // Binary resolution: the bare tool name on `$PATH`, overridable via
    // `WORKSTATE_<TOOL>_BIN` for a user whose binaries live somewhere non-standard
    // (a sibling repo's `target/release`, say) without hardcoding any layout here.
    // A tool that is not installed is NOT an error: the transport returns `NotFound`
    // and the compiler degrades that one section to `Missing` — same graceful story
    // as a missing fixture, now keyed on "tool present?" instead of "file present?".
    let builder = SnapshotBuilder::new(
        BulwarkFeed::from_command(&tool_bin("BULWARK", "bulwark"), &["workstate-feed"]),
        ScriptVaultFeed::from_command(&tool_bin("SCRIPTVAULT", "scriptvault"), &["workstate-feed"]),
        ToolFoundryFeed::from_command(&tool_bin("TOOLFOUNDRY", "toolfoundry"), &["workstate-feed"]),
        ProtoFeed::from_command(&tool_bin("PROTO", "proto"), &["workstate-feed"]),
    );

    // -- Compile --------------------------------------------------------------
    // `build()` returns a `Snapshot` directly (NOT a Result): a degraded build is
    // still a valid snapshot, so there is no error to handle here. Any missing or
    // malformed feed has already become a Missing/Failed section inside it.
    let snapshot = builder.build();

    // -- Publish --------------------------------------------------------------
    // Serialize + write to disk. This is the one fallible step in main (the file
    // system can refuse), so we attach human context and let `?` report-and-exit.
    write_snapshot(&snapshot, &output_path)
        .with_context(|| format!("writing snapshot to {}", output_path.display()))?;

    // -- Summarize ------------------------------------------------------------
    // A single, scannable block proving the run did real work: where it wrote, how
    // many scripts were ingested, and the status of every section. This is what
    // makes the graceful-degradation story visible without opening the JSON.
    //
    // PRINTED TO STDERR, NOT STDOUT. The snapshot itself may BE stdout: the
    // documented `workstate /dev/stdout | rexops` flow streams the JSON to stdout
    // so the consumer can read it from a pipe. If this human summary also went to
    // stdout it would be appended right after the JSON and corrupt that stream.
    // Convention: data on stdout, diagnostics on stderr — so the summary still
    // shows on the terminal during a normal run, but never pollutes the pipe.
    // Per-section record count, taken from the data when present (Missing / Failed /
    // UnsupportedVersion sections have no data, so they count 0).
    let script_count = snapshot
        .scripts
        .data
        .as_ref() // borrow the Option<ScriptInventory> without consuming it
        .map_or(0, |inventory| inventory.scripts.len());
    let tool_count = snapshot
        .tools
        .data
        .as_ref()
        .map_or(0, |inventory| inventory.tools.len());
    let finding_count = snapshot
        .findings
        .data
        .as_ref()
        .map_or(0, |inventory| inventory.findings.len());
    let job_count = snapshot
        .jobs
        .data
        .as_ref()
        .map_or(0, |inventory| inventory.jobs.len());

    eprintln!("Workstate - Central State Compiler for Linux Ops Suite");
    eprintln!("Wrote snapshot -> {}", output_path.display());
    eprintln!(
        "  scripts:  {}",
        section_summary(
            &snapshot.scripts.status,
            script_count,
            snapshot.scripts.provenance.dropped_records
        )
    );
    eprintln!(
        "  tools:    {}",
        section_summary(
            &snapshot.tools.status,
            tool_count,
            snapshot.tools.provenance.dropped_records
        )
    );
    eprintln!(
        "  findings: {}",
        section_summary(
            &snapshot.findings.status,
            finding_count,
            snapshot.findings.provenance.dropped_records
        )
    );
    eprintln!(
        "  jobs:     {}",
        section_summary(
            &snapshot.jobs.status,
            job_count,
            snapshot.jobs.provenance.dropped_records
        )
    );

    Ok(()) // success exit code
}

/// Build one section's summary line: status, kept-record count, and — when any
/// were dropped — the dropped count. The dropped suffix only appears when it is
/// non-zero so a clean section stays quiet, but a lossy one is impossible to miss
/// (it surfaces the silent-data-loss that the count on `Provenance` now records).
fn section_summary(status: &FeedStatus, kept: usize, dropped: usize) -> String {
    if dropped > 0 {
        format!(
            "{}  ({kept} record(s), {dropped} dropped)",
            status_label(status)
        )
    } else {
        format!("{}  ({kept} record(s))", status_label(status))
    }
}

/// Render a `FeedStatus` as a short word for the summary line.
///
/// Dedicated formatting helper: `Failed { reason }`'s `Debug`
/// would dump the whole reason string mid-line; here we want a compact label and
/// surface the reason separately. Taking `&Section`'s status by reference avoids
/// moving anything out of the snapshot we are about to keep using.
fn status_label(status: &FeedStatus) -> String {
    match status {
        FeedStatus::Fresh => "Fresh".to_string(),
        FeedStatus::Stale => "Stale".to_string(),
        // Read OK, but the source's age is unknown, so we can't claim it's fresh.
        FeedStatus::FreshnessUnknown => "FreshnessUnknown".to_string(),
        FeedStatus::UnsupportedVersion { found, supported } => match found {
            Some(found) => format!("UnsupportedVersion ({found}; expected {supported})"),
            None => format!("UnsupportedVersion (missing; expected {supported})"),
        },
        // No schema version declared at all — kept distinct from a wrong version.
        FeedStatus::MissingVersion { supported } => {
            format!("MissingVersion (none declared; expected {supported})")
        }
        // Feed's self-reported source_tool disagreed with the adapter's expectation.
        FeedStatus::SourceMismatch { expected, found } => {
            format!("SourceMismatch (got '{found}'; expected '{expected}')")
        }
        FeedStatus::Missing => "Missing".to_string(),
        // Include the reason for Failed so a broken feed is diagnosable from the
        // one-line summary without opening snapshot.json.
        FeedStatus::Failed { reason } => format!("Failed ({reason})"),
        // FeedStatus is #[non_exhaustive]: a future status variant we don't yet
        // label degrades to its Debug form rather than failing the build.
        other => format!("{other:?}"),
    }
}
