// logging.rs — the one place that installs a `tracing` subscriber (core only
// emits; the frontend decides where events go). Two destinations with opposite
// constraints: the CLI logs to STDERR (stdout stays clean for piping); the TUI
// owns the alternate screen, so it logs to a FILE instead. `RUST_LOG` always
// wins; otherwise the baseline is WARN and `--verbose` raises our crates to DEBUG.

use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

/// `EnvFilter` from `RUST_LOG`, else `warn` (plus our crates at `debug` when
/// `--verbose`, so per-file skips show without drowning in dependency logs).
fn filter(verbose: bool) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if verbose {
            EnvFilter::new("warn,scriptvault=debug,scriptvault_core=debug")
        } else {
            EnvFilter::new("warn")
        }
    })
}

/// Init CLI logging: compact events to STDERR (no timestamps/targets).
pub fn init_cli(verbose: bool) {
    // `try_init` so a double-init can't panic the binary.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter(verbose))
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .try_init();
}

/// Init TUI logging: events appended to a log FILE, but only when requested
/// (`--verbose` or `RUST_LOG`) — otherwise no stray empty log on every launch.
/// Returns the path so the caller can tell the user where to look; `None` =
/// logging stayed off, and a logging failure never blocks the TUI from starting.
pub fn init_tui(verbose: bool) -> Option<PathBuf> {
    let env_requested = std::env::var_os("RUST_LOG").is_some();
    if !verbose && !env_requested {
        return None;
    }

    let path = log_file_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    // Append so successive runs accumulate (rotation is out of scope; the user
    // can delete the file).
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter(verbose))
        .with_writer(file)
        .with_ansi(false) // a file has no terminal
        .try_init();

    Some(path)
}

/// `<data-dir>/scriptvault/scriptvault.log` (same app folder as the state file).
fn log_file_path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("scriptvault").join("scriptvault.log"))
}
