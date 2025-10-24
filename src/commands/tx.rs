use super::CommandRunner;
use crate::{cli, context};
use anyhow::Result;

impl CommandRunner for cli::TxCmd {
    fn run(&self, _ctx: &context::Context) -> Result<()> {
        todo!()
    }
}
