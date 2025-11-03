use super::CommandRunner;
use crate::{cli, context};
use anyhow::Result;

impl CommandRunner for cli::TxCmd {
    fn run(&self, _ctx: &context::Context) -> Result<()> {
        match self {
            cli::TxCmd::RegisterCollection { .. } => {
                // existing register collection will be implemented elsewhere
                todo!("RegisterCollection not implemented yet")
            }
            cli::TxCmd::SendAmount { .. } => {
                // call into txs::send_amount when implemented
                todo!("SendAmount not implemented yet")
            }
        }
    }
}
