use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

// The per-command handlers, each in its own small file (no god-file).
mod autocheck; // bare `proto` in a project — detect language, pick/run check profile
mod delete; // `proto delete <id>` — remove a saved session
mod export; // `proto export <id>` — render a session as markdown/json
mod feed_cmd; // `proto feed` — regenerate the Workstate feed
mod list;
mod picker;
mod run;
mod search; // `proto search <query>` — find sessions by id/title/note
mod sessions;
mod show;
mod validate;

use crate::core::store;

// Default location of the protocols directory when --dir isn't given. Normal
// subcommands keep using `./protocols`; bare interactive launch may fall back to
// the bundled examples when launched from another project.
const DEFAULT_PROTOCOLS_DIR: &str = "protocols";

// -----------------------------------------------------------------------------
// Cli — the top-level parser.
// -----------------------------------------------------------------------------
// `#[derive(Parser)]` turns this struct into a full argv parser with --help and
// --version generated from Cargo.toml metadata.
#[derive(Debug, Parser)]
#[command(
    name = "proto",
    about = "Guided protocol / checklist runner for the Linux Ops Suite",
    version // pull the version string from Cargo.toml
)]
pub struct Cli {
    // A GLOBAL option (usable before or after the subcommand) pointing at the
    // protocols directory. `global = true` means `proto --dir x list` and
    // `proto list --dir x` both work. None means use the command's default.
    #[arg(
        long,
        global = true,
        help = "Directory containing protocol *.yaml files (default: ./protocols)"
    )]
    pub dir: Option<PathBuf>,

    // A GLOBAL option for where session records are read/written. Unlike --dir we
    // can't give a static default_value, because the default is COMPUTED at
    // runtime (XDG_DATA_HOME/proto/sessions → ~/.proto/sessions). So it's an
    // Option: None means "use store::default_dir()", resolved in run() below.
    #[arg(
        long,
        global = true,
        help = "Directory for session records (default: $XDG_DATA_HOME/proto/sessions or ~/.proto/sessions)"
    )]
    pub sessions_dir: Option<PathBuf>,

    // Where the Workstate feed is written. Like --sessions-dir the default is
    // COMPUTED at runtime ($XDG_DATA_HOME/workstate/feeds), so it's an Option:
    // None => use store::feed_default_dir(). Overriding it is useful for tests and
    // for pointing the feed at a non-standard Workstate location.
    #[arg(
        long,
        global = true,
        help = "Directory for the Workstate feed (default: $XDG_DATA_HOME/workstate/feeds)"
    )]
    pub feed_dir: Option<PathBuf>,

    // Suppress the automatic feed write after a run. By default `proto run`
    // regenerates the Workstate feed so the suite stays current with no extra
    // step; --no-feed opts out (e.g. a throwaway run you don't want surfaced).
    #[arg(
        long,
        global = true,
        help = "Do not (re)write the Workstate feed after a run"
    )]
    pub no_feed: bool,

    // The chosen subcommand. Running bare `proto` prints help (handled in run()).
    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    // Resolve the protocols directory for explicit protocol commands. These keep
    // the historical `./protocols` default, including its error if missing.
    fn protocols_dir(&self) -> PathBuf {
        self.dir.clone().unwrap_or_else(default_protocols_dir)
    }

    // Resolve the protocols directory for bare interactive launch. When the user
    // did not pass --dir and the current project has no local protocols folder,
    // use the protocols bundled with this crate so the legacy picker still works
    // from Go/Python/Node/etc. projects.
    fn bare_protocols_dir(&self) -> PathBuf {
        bare_protocols_dir_from(
            self.dir.as_deref(),
            &default_protocols_dir(),
            &bundled_protocols_dir(),
        )
    }

    // Resolve the effective sessions directory: the --sessions-dir override if
    // given, else the computed default. One helper so every command agrees.
    fn sessions_dir(&self) -> PathBuf {
        self.sessions_dir.clone().unwrap_or_else(store::default_dir)
    }

    // Resolve the effective feed directory: --feed-dir override, else the computed
    // Workstate feeds default. Same pattern as sessions_dir, one source of truth.
    fn feed_dir(&self) -> PathBuf {
        self.feed_dir
            .clone()
            .unwrap_or_else(store::feed_default_dir)
    }
}

fn default_protocols_dir() -> PathBuf {
    PathBuf::from(DEFAULT_PROTOCOLS_DIR)
}

fn bundled_protocols_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_PROTOCOLS_DIR)
}

fn bare_protocols_dir_from(
    configured: Option<&Path>,
    cwd_default: &Path,
    bundled: &Path,
) -> PathBuf {
    if let Some(configured) = configured {
        return configured.to_path_buf();
    }

    if cwd_default.is_dir() {
        return cwd_default.to_path_buf();
    }

    if bundled.is_dir() {
        return bundled.to_path_buf();
    }

    cwd_default.to_path_buf()
}

// -----------------------------------------------------------------------------
// Command — the subcommands. One variant per `proto <verb>`.
// -----------------------------------------------------------------------------
#[derive(Debug, Subcommand)]
pub enum Command {
    // `proto list` — show every valid protocol in the directory.
    #[command(about = "List available protocols")]
    List,

    // `proto validate [id]` — check protocols obey the rules. With an id, check
    // just that one; without, check (and report on) all of them.
    #[command(about = "Validate one or all protocols")]
    Validate {
        // Optional positional id. None => validate everything.
        #[arg(help = "Protocol id to validate (omit to validate all)")]
        id: Option<String>,
    },

    // `proto run <id>` — walk the operator through a protocol interactively.
    #[command(about = "Run a protocol interactively")]
    Run {
        // Required positional: which protocol to run.
        #[arg(help = "Protocol id to run")]
        id: String,
    },

    // `proto sessions` — list past runs (id, protocol, when, outcome tally).
    #[command(about = "List past session records")]
    Sessions,

    // `proto show <session-id>` — print the full detail of one past run.
    #[command(about = "Show one session record in detail")]
    Show {
        // Required positional: the session id (the filename stem from `sessions`).
        #[arg(help = "Session id to show (see `proto sessions`)")]
        id: String,
    },

    // `proto feed` — regenerate the Workstate feed from saved sessions on demand.
    // `run` does this automatically; this command is for a manual/cron refresh, or
    // to rebuild the feed after deleting a session.
    #[command(about = "Regenerate the Workstate feed from saved sessions")]
    Feed,

    // `proto delete <id>` — remove one saved session (confirms unless --yes).
    #[command(about = "Delete a saved session record")]
    Delete {
        #[arg(help = "Session id to delete (see `proto sessions`)")]
        id: String,
        // Skip the confirmation prompt — for scripts or a decided user.
        #[arg(long, help = "Delete without confirmation")]
        yes: bool,
    },

    // `proto search <query>` — case-insensitive substring over saved sessions.
    #[command(about = "Search saved sessions by protocol or note text")]
    Search {
        #[arg(help = "Text to search for (matches protocol id/title and step notes)")]
        query: String,
    },

    // `proto export <id>` — render one session as Markdown (default) or JSON, to
    // stdout or a file.
    #[command(about = "Export a session as Markdown or JSON")]
    Export {
        #[arg(help = "Session id to export (see `proto sessions`)")]
        id: String,
        // Format flags. They conflict (you pick one); clap enforces that via the
        // group below. Neither given => Markdown (the common, human-facing case).
        #[arg(long, help = "Export as Markdown (default)")]
        markdown: bool,
        #[arg(long, conflicts_with = "markdown", help = "Export as raw JSON")]
        json: bool,
        // Optional output file; omitted => write to stdout (pipeable).
        #[arg(long, value_name = "FILE", help = "Write to a file instead of stdout")]
        out: Option<PathBuf>,
    },
}

// -----------------------------------------------------------------------------
// run — dispatch the parsed Cli to the matching handler.
// -----------------------------------------------------------------------------
// This is the single entry point main.rs calls. It matches on the subcommand and
// forwards the protocols directory plus any args. Returning `anyhow::Result`
// lets handlers use `?` freely; main.rs turns an Err into a friendly exit.
pub fn run(cli: Cli) -> anyhow::Result<()> {
    // Both directories are shared by the commands, so resolve them once. `dir` is
    // the protocols dir (static default); `sessions` is the computed store dir.
    let dir = cli.protocols_dir();
    let sessions = cli.sessions_dir();
    let feed = cli.feed_dir();
    // Whether `run` should (re)write the feed afterwards. Inverted from the flag:
    // the default is to write, --no-feed opts out.
    let write_feed = !cli.no_feed;

    match cli.command {
        // Bare `proto` with no subcommand. The action depends on context:
        //   * On a real terminal (interactive) -> auto-detect the current project
        //     and offer check profiles. If no project is detected, fall back to
        //     the protocol picker so the older guided-checklist flow stays alive.
        //   * Non-interactive (piped, no TTY, a script) -> print help, so
        //     `proto | ...` and CI stay predictable and don't block on a prompt.
        None => {
            use std::io::IsTerminal; // std (since 1.70) — no extra dependency
            if std::io::stdin().is_terminal() {
                let bare_dir = cli.bare_protocols_dir();
                autocheck::handle_or_picker(&bare_dir, &sessions, &feed, write_feed)
            } else {
                use clap::CommandFactory; // brings `Cli::command()` into scope
                Cli::command().print_help()?; // write the help text to stdout
                println!(); // trailing newline so the prompt isn't glued to help
                Ok(())
            }
        }
        Some(Command::List) => list::handle(&dir),
        Some(Command::Validate { id }) => validate::handle(&dir, id.as_deref()),
        Some(Command::Run { id }) => run::handle(&dir, &sessions, &feed, write_feed, &id),
        Some(Command::Sessions) => sessions::handle(&sessions),
        Some(Command::Show { id }) => show::handle(&sessions, &id),
        Some(Command::Feed) => feed_cmd::handle(&sessions, &feed),
        Some(Command::Delete { id, yes }) => delete::handle(&sessions, &feed, write_feed, &id, yes),
        Some(Command::Search { query }) => search::handle(&sessions, &query),
        Some(Command::Export {
            id,
            markdown: _,
            json,
            out,
        }) => {
            // --json picks JSON; otherwise Markdown (whether --markdown was given
            // or no flag at all — Markdown is the default). The `markdown` flag is
            // accepted for explicitness but doesn't need reading: not-json => md.
            let format = if json {
                export::Format::Json
            } else {
                export::Format::Markdown
            };
            export::handle(&sessions, &id, format, out.as_deref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn bare_protocols_dir_uses_configured_dir_first() {
        let root = tempdir().unwrap();
        let configured = root.path().join("custom");
        let cwd_default = root.path().join("protocols");
        let bundled = root.path().join("bundled");
        std::fs::create_dir(&cwd_default).unwrap();
        std::fs::create_dir(&bundled).unwrap();

        assert_eq!(
            bare_protocols_dir_from(Some(&configured), &cwd_default, &bundled),
            configured
        );
    }

    #[test]
    fn bare_protocols_dir_prefers_local_default_when_present() {
        let root = tempdir().unwrap();
        let cwd_default = root.path().join("protocols");
        let bundled = root.path().join("bundled");
        std::fs::create_dir(&cwd_default).unwrap();
        std::fs::create_dir(&bundled).unwrap();

        assert_eq!(
            bare_protocols_dir_from(None, &cwd_default, &bundled),
            cwd_default
        );
    }

    #[test]
    fn bare_protocols_dir_falls_back_to_bundled_protocols() {
        let root = tempdir().unwrap();
        let cwd_default = root.path().join("missing-protocols");
        let bundled = root.path().join("bundled");
        std::fs::create_dir(&bundled).unwrap();

        assert_eq!(
            bare_protocols_dir_from(None, &cwd_default, &bundled),
            bundled
        );
    }

    #[test]
    fn command_protocols_dir_keeps_relative_default() {
        let cli = Cli {
            dir: None,
            sessions_dir: None,
            feed_dir: None,
            no_feed: false,
            command: Some(Command::List),
        };

        assert_eq!(cli.protocols_dir(), PathBuf::from(DEFAULT_PROTOCOLS_DIR));
    }
}
