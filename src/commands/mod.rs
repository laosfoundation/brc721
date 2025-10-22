use crate::cli;
use crate::Result;

pub mod tx;
pub mod wallet;

pub trait CommandRunner {
    fn run(&self, cli: &cli::Cli) -> anyhow::Result<()>;
}

impl cli::Command {
    pub fn run(&self, cli: &cli::Cli) -> Result<()> {
        match self {
            cli::Command::Wallet { cmd } => cmd.run(cli),
            cli::Command::Tx { cmd } => cmd.run(cli),
        }
    }
}
