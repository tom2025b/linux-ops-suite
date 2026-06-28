// ============================================================================
// crates/scriptvault/src/cli.rs
// ============================================================================
// The headless `search` command — THE verification checkpoint for the core.
// It builds a `ScriptVault` (optionally targeting a specific `--root`), runs a
// search through the public facade, and prints results. It touches NO TUI code,
// demonstrating the core facade is fully usable by a plain CLI consumer.
// ============================================================================

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, ValueEnum};
use scriptvault_core::{Config, Filter, MatchField, Query, ScriptVault, SearchResult, Sort};

mod export;
mod generate;
mod output;
mod workstate_feed;

/// How `search` prints its results.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable aligned table (the default).
    #[default]
    Table,
    /// One JSON array of {name, lang, path, tags, desc} — pipe to a file to save.
    Json,
    /// CSV with a header row (name,lang,path,tags,desc); tags joined by `;`.
    Csv,
}

/// Result ordering for `--sort`. Maps onto the core engine's `Sort`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum SortArg {
    /// The hybrid: fuzzy match quality, then frecency, then name (the default).
    #[default]
    Auto,
    /// Alphabetical by display name.
    Name,
    /// Most recently run first.
    Recent,
    /// Most recently modified on disk first.
    Modified,
}

impl From<SortArg> for Sort {
    fn from(s: SortArg) -> Self {
        match s {
            SortArg::Auto => Sort::Auto,
            SortArg::Name => Sort::Name,
            SortArg::Recent => Sort::RecentlyRun,
            SortArg::Modified => Sort::Modified,
        }
    }
}

// Re-export the `gen` subcommand surface so main.rs uses `cli::GenArgs` /
// `cli::run_gen`, matching how `SearchArgs` / `run_search` are exposed here.
// (The module is `generate`, not `gen`, because `gen` is a reserved keyword in
// the 2024 edition — the user-facing subcommand name is still `gen`.)
pub use generate::{GenArgs, run_gen};

// Same exposure pattern for the `workstate-feed` subcommand: main.rs reaches it
// as `cli::WorkstateFeedArgs` / `cli::run_workstate_feed`, mirroring search/gen.
pub use workstate_feed::{WorkstateFeedArgs, run as run_workstate_feed};

/// Arguments for `scriptvault search`.
#[derive(Debug, Args)]
pub struct SearchArgs {
    /// The fuzzy query. Omit (or pass empty) to list everything.
    #[arg(default_value = "")]
    pub query: String,

    /// Search only this directory instead of the configured roots. Repeatable.
    /// Useful for trying ScriptVault against a specific folder without editing your
    /// config — and the seam tests use it to point at a fixture tree.
    #[arg(long = "root", value_name = "DIR")]
    pub roots: Vec<PathBuf>,

    /// Print only file paths, one per line (pipe-friendly:
    /// `scriptvault search foo --paths-only | xargs ...`).
    #[arg(long = "paths-only")]
    pub paths_only: bool,

    /// Output format: `table` (default, human-readable), `json`, or `csv`.
    /// Pipe `json`/`csv` to a file to save a scan: `… --format json > out.json`.
    /// (`--paths-only` takes precedence if both are given.)
    #[arg(long, value_enum, default_value_t)]
    pub format: OutputFormat,

    /// Limit the number of results shown (0 = no limit).
    #[arg(long, default_value_t = 0)]
    pub limit: usize,

    /// Keep only scripts carrying this tag (case-insensitive). Repeatable; all
    /// must match. Equivalent to `t:<TAG>` in the query string.
    #[arg(long = "tag", value_name = "TAG")]
    pub tags: Vec<String>,

    /// Keep only scripts of this language, e.g. `--lang bash` (aliases like
    /// `py`/`sh` are accepted). Equivalent to `lang:<LANG>` in the query.
    #[arg(long, value_name = "LANG")]
    pub lang: Option<String>,

    /// Keep only favorited scripts. Equivalent to `fav:` in the query.
    #[arg(long)]
    pub fav: bool,

    /// Result ordering: `auto` (hybrid, default), `name`, `recent`, `modified`.
    #[arg(long, value_enum, default_value_t)]
    pub sort: SortArg,

    /// Run the top-matching script instead of printing results. If the query
    /// matches several, only the best-ranked one runs (a note names it) — narrow
    /// the query to target another. The script inherits this terminal; a non-zero
    /// exit becomes a non-zero exit here. `--exec` is an alias.
    #[arg(long, visible_alias = "exec")]
    pub run: bool,

    /// Show diagnostics for files that looked like scripts (known extension or
    /// `#!` shebang) but couldn't be read as text. Off by default to keep output
    /// clean; non-script files (binaries, data) are always skipped silently.
    #[arg(long)]
    pub verbose: bool,
}

/// Run the search command end-to-end.
pub fn run_search(args: SearchArgs) -> Result<()> {
    // Install the CLI logger (stderr) before building the engine, since the
    // scan/parse that emit diagnostics happen inside `load`. `--verbose` raises
    // our crates to DEBUG; `RUST_LOG` overrides. Baseline WARN still shows the
    // always-on sidecar/state warnings, exactly as the old eprintln path did.
    crate::logging::init_cli(args.verbose);

    // Build the engine. If the user passed --root, construct a Config targeting
    // exactly those roots (with sensible default ignores); otherwise load the
    // user's normal configuration.
    let scriptvault = if args.roots.is_empty() {
        ScriptVault::load()?
    } else {
        ScriptVault::load_with(config_for_roots(args.roots.clone())?)?
    };

    // Build a structured Query from the query string + explicit flags, then run
    // it through the SAME engine the TUI uses. This is what gives the headless
    // CLI real filtering: `search "t:ci deploy"` now filters (the query string is
    // parsed by core), and `--tag/--lang/--fav/--sort/--limit` build the rest.
    let query = build_query(&args);
    let results = scriptvault.query(&query);

    if results.is_empty() {
        // Empty result is valid, not an error: report on stderr, leave stdout
        // clean, exit 0.
        eprintln!("scriptvault: no matches for {:?}", args.query);
        return Ok(());
    }

    // `--run` is an ACTION, not a listing: run the top match and return instead of
    // printing the table. Done before print_results so `--run` ignores --format etc.
    if args.run {
        return run_top_match(&results);
    }

    print_results(
        &results,
        args.paths_only,
        args.format,
        output::ColorChoice::from_stdout(std::io::stdout().is_terminal()),
    );
    Ok(())
}

/// Build a structured [`Query`] from the parsed CLI args. The query STRING is
/// parsed by core (so inline operators like `t:ci`/`lang:bash` work headless),
/// then the explicit flags (`--tag`/`--lang`/`--fav`/`--sort`/`--limit`) are
/// layered on top. Unknown `--lang` values are dropped (forgiving, like the
/// query parser), so a typo never errors a pipeline.
fn build_query(args: &SearchArgs) -> Query {
    let mut q = scriptvault_core::parse_query(&args.query);

    for tag in &args.tags {
        q.filters.push(Filter::Tag(tag.to_lowercase()));
    }
    if let Some(lang) = args.lang.as_deref()
        && let Some(l) = scriptvault_core::Language::from_label(lang)
    {
        q.filters.push(Filter::Lang(l));
    }
    if args.fav {
        q.filters.push(Filter::Favorite);
    }
    q.sort = args.sort.into();
    // `--limit 0` means "no limit"; map it to None so the engine returns all.
    q.limit = (args.limit > 0).then_some(args.limit);
    q
}

/// Construct a Config that scans exactly the given roots, reusing the embedded
/// default ignore set so noise (.git, node_modules, …) is still pruned.
fn config_for_roots(roots: Vec<PathBuf>) -> Result<Config> {
    // Start from defaults to inherit ignores/editor, then override roots. If the
    // user config is malformed, keep the shipped defaults instead of falling
    // back to Config::default() (which would lose `.git`/node_modules ignores).
    let mut config = match Config::load() {
        Ok(config) => config,
        Err(err) => {
            tracing::warn!(%err, "ignoring user config for explicit --root scan");
            Config::defaults()?
        }
    };
    config.roots = roots;
    Ok(config)
}

/// Run the single best-ranked match. If the query matched several, we run ONLY
/// the top one (never fire multiple scripts from one fuzzy query) and say so on
/// stderr so the user can narrow the query to target another. The script inherits
/// this terminal (full stdin/stdout/stderr) via core's `actions::run`, and its
/// non-zero exit propagates out as our error (so `&&` chaining behaves).
fn run_top_match(results: &[SearchResult]) -> Result<()> {
    // `results` is non-empty here (checked by the caller), so [0] is the top rank.
    let top = &results[0];
    let name = top.entry.display_name();

    // Notes go to STDERR so they never pollute a script's stdout (which a caller
    // may be capturing). Only mention the ambiguity when there actually is any.
    if results.len() > 1 {
        eprintln!(
            "scriptvault: matched {} scripts, running the top match: {} ({})",
            results.len(),
            name,
            top.entry.path.display()
        );
        eprintln!("scriptvault: narrow your query to run a different one");
    } else {
        eprintln!(
            "scriptvault: running {} ({})",
            name,
            top.entry.path.display()
        );
    }

    // Delegate to the SAME core action the TUI's ^R uses. A non-zero script exit
    // returns Err, which main() surfaces and turns into a non-zero process exit.
    scriptvault_core::actions::run(&top.entry)?;
    Ok(())
}

/// Print results in the requested shape. `--paths-only` is the most specific
/// request, so it wins over `--format`; otherwise the chosen format decides.
fn print_results(
    results: &[SearchResult],
    paths_only: bool,
    format: OutputFormat,
    color: output::ColorChoice,
) {
    if paths_only {
        for r in results {
            println!("{}", r.entry.path.display());
        }
        return;
    }

    match format {
        OutputFormat::Table => print!("{}", output::render_results_table(results, color)),
        // json/csv go to stdout verbatim (newline-terminated) so `> file` is clean.
        OutputFormat::Json => println!("{}", export::to_json(results)),
        OutputFormat::Csv => print!("{}", export::to_csv(results)),
    }
}

pub(crate) fn match_label(field: MatchField) -> &'static str {
    match field {
        MatchField::Name => "name",
        MatchField::Tags => "tags",
        MatchField::Desc => "desc",
        MatchField::Filename => "file",
    }
}

// ============================================================================
// Tests — exercise the FULL pipeline through the binary's own code path,
// against a reproducible temp fixture tree (only possible thanks to load_with).
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture_tree() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("scriptvault-cli-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        // A: explicit name "deploy".
        fs::write(
            dir.join("a.sh"),
            "#!/bin/bash\n# scriptvault.name: deploy\n# scriptvault.desc: ship it\necho a\n",
        )
        .unwrap();
        // B: name unrelated, desc mentions deploy.
        fs::write(
            dir.join("b.sh"),
            "#!/bin/bash\n# scriptvault.name: unrelated\n# scriptvault.desc: deploy the thing\necho b\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn full_pipeline_via_facade_ranks_name_first() {
        let dir = fixture_tree();
        // Drive the SAME facade the CLI uses, against a known fixture dir.
        let scriptvault =
            ScriptVault::load_with(config_for_roots(vec![dir.clone()]).unwrap()).unwrap();
        let results = scriptvault.search("deploy");

        assert!(results.len() >= 2);
        // Tiered ranking: the name="deploy" script ranks above the desc match.
        assert_eq!(results[0].entry.display_name(), "deploy");
        assert_eq!(results[0].matched_field, MatchField::Name);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_query_lists_all_fixtures() {
        let dir = fixture_tree();
        let scriptvault =
            ScriptVault::load_with(config_for_roots(vec![dir.clone()]).unwrap()).unwrap();
        let results = scriptvault.search("");
        assert_eq!(results.len(), 2);
        fs::remove_dir_all(&dir).ok();
    }

    /// Build a SearchArgs with everything defaulted except what a test sets.
    fn args(query: &str) -> SearchArgs {
        SearchArgs {
            query: query.to_string(),
            roots: vec![],
            paths_only: false,
            format: OutputFormat::Table,
            limit: 0,
            tags: vec![],
            lang: None,
            fav: false,
            sort: SortArg::Auto,
            run: false,
            verbose: false,
        }
    }

    #[test]
    fn build_query_parses_inline_operators_from_the_query_string() {
        // The headline P2 CLI win: `search "t:ci deploy"` now FILTERS headless.
        let q = build_query(&args("t:ci deploy"));
        assert_eq!(q.text, "deploy");
        assert!(q.filters.contains(&Filter::Tag("ci".into())));
    }

    #[test]
    fn build_query_layers_explicit_flags_on_top() {
        let mut a = args("deploy");
        a.tags = vec!["ops".into()];
        a.lang = Some("bash".into());
        a.fav = true;
        a.limit = 5;
        a.sort = SortArg::Name;
        let q = build_query(&a);
        assert!(q.filters.contains(&Filter::Tag("ops".into())));
        assert!(
            q.filters
                .contains(&Filter::Lang(scriptvault_core::Language::Bash))
        );
        assert!(q.filters.contains(&Filter::Favorite));
        assert_eq!(q.limit, Some(5));
        assert_eq!(q.sort, Sort::Name);
    }

    #[test]
    fn build_query_drops_unknown_lang_flag() {
        // Forgiving: a bad --lang value adds no filter (never errors a pipeline).
        let mut a = args("");
        a.lang = Some("cobol".into());
        assert!(build_query(&a).filters.is_empty());
    }

    #[test]
    fn build_query_zero_limit_means_no_limit() {
        assert_eq!(build_query(&args("")).limit, None);
    }

    #[test]
    fn cli_query_filters_a_real_scan_by_tag() {
        // End-to-end through the facade: a fixture tagged "ci" is found by t:ci,
        // and the untagged one is excluded.
        let dir = fixture_tree();
        // Add a ci tag to a.sh's deploy script via a third file with a tag.
        fs::write(
            dir.join("c.sh"),
            "#!/bin/bash\n# scriptvault.name: cibuild\n# scriptvault.tags: ci\necho c\n",
        )
        .unwrap();
        let sv = ScriptVault::load_with(config_for_roots(vec![dir.clone()]).unwrap()).unwrap();
        let out = sv.query(&build_query(&args("t:ci")));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].entry.display_name(), "cibuild");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn format_json_serializes_a_real_scan_to_valid_json() {
        // The end-to-end flag path: scan a real fixture dir, then run the json
        // serializer the `--format json` arm calls. Proves `search "" --root <dir>
        // --format json` produces a parseable array of the scanned scripts.
        let dir = fixture_tree();
        let scriptvault =
            ScriptVault::load_with(config_for_roots(vec![dir.clone()]).unwrap()).unwrap();
        let results = scriptvault.search("");

        let json = export::to_json(&results);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 2);
        // Every row carries the user-facing fields and a real path.
        for row in parsed.as_array().unwrap() {
            assert!(row["name"].is_string());
            assert!(row["path"].as_str().unwrap().ends_with(".sh"));
        }

        // And CSV has its header + one line per result.
        let csv = export::to_csv(&results);
        assert_eq!(csv.lines().next().unwrap(), "name,lang,path,tags,desc");
        assert_eq!(csv.lines().count(), 3); // header + 2 rows

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_top_match_runs_best_and_propagates_exit() {
        // A fixture with two scripts: the top-ranked one exits 0, another exits 3.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("scriptvault-run-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.sh");
        fs::write(&ok, "#!/bin/sh\n# scriptvault.name: deploy\nexit 0\n").unwrap();
        let bad = dir.join("bad.sh");
        fs::write(&bad, "#!/bin/sh\n# scriptvault.name: deployfail\nexit 3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for p in [&ok, &bad] {
                fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }

        let sv = ScriptVault::load_with(config_for_roots(vec![dir.clone()]).unwrap()).unwrap();

        // "deploy" ranks the exit-0 script first -> running the top match is Ok.
        let ok_results = sv.search("deploy");
        assert_eq!(ok_results[0].entry.display_name(), "deploy");
        assert!(run_top_match(&ok_results).is_ok());

        // Target the failing script directly -> its non-zero exit propagates Err.
        let bad_results = sv.search("deployfail");
        assert_eq!(bad_results[0].entry.display_name(), "deployfail");
        assert!(
            run_top_match(&bad_results).is_err(),
            "a non-zero script exit must surface as an error"
        );

        fs::remove_dir_all(&dir).ok();
    }
}

// CLI uses only core + clap (no TUI). Tests exercise the full pipeline via facade.
