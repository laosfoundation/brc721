use crate::cli;

pub mod wallet;

pub trait CommandRunner {
    fn run(&self, cli: &cli::Cli) -> anyhow::Result<()>;
}
