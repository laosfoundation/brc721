use std::str::FromStr;

use super::CommandRunner;
use crate::{cli, context, wallet::brc721_wallet::Brc721Wallet};
use anyhow::Result;
use bitcoin::{Address, Amount};

impl CommandRunner for cli::TxCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::TxCmd::RegisterCollection { .. } => {
                // existing register collection will be implemented elsewhere
                todo!("RegisterCollection not implemented yet")
            }
            cli::TxCmd::SendAmount {
                to,
                amount_sat,
                fee_rate,
            } => {
                let wallet = Brc721Wallet::load(&ctx.data_dir, ctx.network)?;
                wallet.setup_watch_only(&ctx.rpc_url, ctx.auth.clone())?;
                let amount = Amount::from_sat(*amount_sat);
                let address = Address::from_str(to)?.require_network(ctx.network)?;
                wallet.send_amount(&ctx.rpc_url, ctx.auth.clone(), &address, amount, *fee_rate)?;
                log::info!("✅ Sent {} sat to {}", amount_sat, to);
                Ok(())
            }
        }
    }
}
