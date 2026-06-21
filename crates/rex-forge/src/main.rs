#![forbid(unsafe_code)]
use clap::Parser;
use rex_forge::cli::{Cli, Cmd};
use rex_forge::registry;

fn main() {
    let cli = Cli::parse();
    let reg = registry::load();
    let result = match cli.cmd {
        Cmd::New(args) => rex_forge::run_new(&reg, &args),
        Cmd::List => {
            rex_forge::run_list(&reg);
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(e.exit_code());
    }
}
