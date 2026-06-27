mod args;
mod commands;
mod output;

pub use args::Cli;

impl Cli {
    pub fn run(self) -> anyhow::Result<()> {
        commands::run(self.command)
    }
}
