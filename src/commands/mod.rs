use crate::cli;

pub mod tx;
pub mod wallet;

pub trait CommandRunner {
    fn run(&self, cli: &cli::Cli) -> anyhow::Result<()>;
}

impl cli::Command {
    pub fn run(&self, cli: &cli::Cli) -> anyhow::Result<()> {
        match self {
            cli::Command::Wallet { cmd } => cmd.run(cli),
            cli::Command::Tx { cmd } => cmd.run(cli),
        }
    }
}
