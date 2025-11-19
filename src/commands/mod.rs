use crate::cli::Command;
use crate::context;

pub mod tx;
pub mod wallet;

pub trait CommandRunner {
    fn run(&self, ctx: &context::Context) -> anyhow::Result<()>;
}

impl Command {
    pub fn run(&self, ctx: &context::Context) -> anyhow::Result<()> {
        match self {
            Command::Wallet { cmd } => cmd.run(ctx),
            Command::Tx { cmd } => cmd.run(ctx),
        }
    }
}
