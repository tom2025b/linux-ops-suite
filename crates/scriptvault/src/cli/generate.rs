// ============================================================================
// crates/scriptvault/src/cli/generate.rs   [hidden `gen` subcommand]
// ----------------------------------------------------------------------------
// File is `generate.rs` (not `gen.rs`) because `gen` is a reserved keyword in
// the Rust 2024 edition; the user-facing subcommand is still `scriptvault gen`.
// ============================================================================
// Generates shell-completion scripts and a man page for ScriptVault straight
// from the live `clap` command definition — so they can never drift out of sync
// with the real flags/subcommands the way hand-written docs do.
//
// Exposed as a HIDDEN subcommand (`scriptvault gen <target>`) so it stays out of
// `--help` (end users don't care; packagers and the install script do). Writes
// to stdout so the caller redirects it where they want, e.g.:
//
//     scriptvault gen zsh  > ~/.zfunc/_scriptvault
//     scriptvault gen bash > /etc/bash_completion.d/scriptvault
//     scriptvault gen man  > scriptvault.1
//
// It touches NO TUI code and no core state — it's pure description-of-the-CLI.
// ============================================================================

use std::io;

use anyhow::Result;
use clap::{Args, CommandFactory, ValueEnum};
use clap_complete::Shell;

/// What to generate. The shells map onto `clap_complete::Shell`; `Man` is the
/// odd one out (a roff man page via `clap_mangen`), so we model the choice as
/// our own enum rather than reusing `Shell` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GenTarget {
    Bash,
    Zsh,
    Fish,
    /// Windows PowerShell completions (harmless to ship on any OS).
    Powershell,
    Elvish,
    /// A roff (`man`) page rendered from the same command tree.
    Man,
}

/// Arguments for `scriptvault gen`.
#[derive(Debug, Args)]
pub struct GenArgs {
    /// Which artifact to print to stdout (a shell name, or `man`).
    #[arg(value_enum)]
    pub target: GenTarget,
}

/// Run the generator: build the command tree once, then emit the requested
/// artifact to stdout. Kept tiny and side-effect-free (beyond writing stdout).
pub fn run_gen(args: GenArgs) -> Result<()> {
    // `command()` reconstructs the full clap `Command` (every flag + the hidden
    // subcommands) from the derived `Cli`. Both generators consume it. We build
    // it once and reuse it; completion generators want `&mut`, mangen takes it
    // by value, so the `man` arm builds its own fresh copy.
    let mut out = io::stdout();
    let bin = "scriptvault"; // the installed binary name (matches [[bin]] in Cargo.toml)

    match args.target {
        GenTarget::Man => render_man(&mut out)?,
        shell => {
            // Map our enum onto clap_complete's Shell, then stream the script.
            let shell = to_clap_shell(shell);
            let mut cmd = crate::Cli::command();
            clap_complete::generate(shell, &mut cmd, bin, &mut out);
        }
    }
    Ok(())
}

/// Translate the non-`Man` variants into `clap_complete::Shell`. Unreachable for
/// `Man` because the caller handles that arm before calling us — but we return a
/// `Result`-free total function and let the caller guarantee the precondition.
fn to_clap_shell(target: GenTarget) -> Shell {
    match target {
        GenTarget::Bash => Shell::Bash,
        GenTarget::Zsh => Shell::Zsh,
        GenTarget::Fish => Shell::Fish,
        GenTarget::Powershell => Shell::PowerShell,
        GenTarget::Elvish => Shell::Elvish,
        // `Man` never reaches here: run_gen branches on it first. If a future
        // edit breaks that invariant, defaulting to Bash is a safe, visible
        // fallback rather than a panic in a doc-generation path.
        GenTarget::Man => Shell::Bash,
    }
}

/// Render the man page (roff) to the given writer using `clap_mangen`.
fn render_man(out: &mut impl io::Write) -> Result<()> {
    let cmd = crate::Cli::command();
    clap_mangen::Man::new(cmd).render(out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: capture a generated artifact into a String for assertions.
    fn generate_to_string(target: GenTarget) -> String {
        let mut buf: Vec<u8> = Vec::new();
        match target {
            GenTarget::Man => render_man(&mut buf).expect("man render"),
            shell => {
                let shell = to_clap_shell(shell);
                let mut cmd = crate::Cli::command();
                clap_complete::generate(shell, &mut cmd, "scriptvault", &mut buf);
            }
        }
        String::from_utf8(buf).expect("generated output is utf-8")
    }

    #[test]
    fn every_shell_emits_nonempty_output_mentioning_the_binary() {
        for target in [
            GenTarget::Bash,
            GenTarget::Zsh,
            GenTarget::Fish,
            GenTarget::Powershell,
            GenTarget::Elvish,
        ] {
            let out = generate_to_string(target);
            assert!(!out.is_empty(), "{target:?} produced empty completion");
            assert!(
                out.contains("scriptvault"),
                "{target:?} completion omits the binary name"
            );
        }
    }

    #[test]
    fn man_page_mentions_binary_and_the_search_subcommand() {
        let man = generate_to_string(GenTarget::Man);
        assert!(!man.is_empty(), "man page is empty");
        // The man page is generated from the real command tree, so the visible
        // `search` subcommand must appear; the hidden `gen` one is allowed to be
        // absent. This is the assertion that proves "docs follow the CLI".
        assert!(man.contains("scriptvault"), "man page omits binary name");
        assert!(
            man.to_lowercase().contains("search"),
            "man page omits the search subcommand"
        );
    }
}
