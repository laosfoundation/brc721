use crate::{cli, context};

pub mod tx;
pub mod wallet;

pub trait CommandRunner {
    fn run(&self, ctx: &context::Context) -> anyhow::Result<()>;
}

impl cli::Command {
    pub fn run(&self, ctx: &context::Context) -> anyhow::Result<()> {
        match self {
            cli::Command::Wallet { cmd } => cmd.run(ctx),
            cli::Command::Tx { cmd } => cmd.run(ctx),
        }
    }
}
