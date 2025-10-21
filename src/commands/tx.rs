use super::CommandRunner;
use crate::cli;
use anyhow::Result;

impl CommandRunner for cli::TxCmd {
    fn run(&self, _cli: &cli::Cli) -> Result<()> {
        todo!()
    }
}
