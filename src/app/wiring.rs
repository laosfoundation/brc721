use std::sync::Arc;

use crate::{context, parser, scanner, storage};
use anyhow::{Context, Result};
use bitcoincore_rpc::Client;

pub fn init_data_dir(ctx: &context::Context) -> Result<()> {
    let data_dir = std::path::PathBuf::from(&ctx.data_dir);
    std::fs::create_dir_all(&data_dir)?;
    Ok(())
}

pub fn init_storage(ctx: &context::Context) -> Result<Arc<dyn storage::Storage + Send + Sync>> {
    let data_dir = std::path::PathBuf::from(&ctx.data_dir);
    let db_path = data_dir
        .join("brc721.sqlite")
        .to_string_lossy()
        .into_owned();
    let sqlite = storage::SqliteStorage::new(&db_path);
    if ctx.reset {
        sqlite.reset_all().context("resetting storage")?;
    }
    sqlite.init().context("initializing storage")?;
    Ok(Arc::new(sqlite))
}

pub fn build_scanner(ctx: context::Context, start_block: u64) -> Result<scanner::Scanner<Client>> {
    let client = Client::new(ctx.rpc_url.as_ref(), ctx.auth.clone())
        .context("failed to create RPC client")?;
    Ok(scanner::Scanner::new(client)
        .with_confirmations(ctx.confirmations)
        .with_capacity(ctx.batch_size)
        .with_start_from(start_block))
}

pub fn build_parser(storage: Arc<dyn storage::Storage + Send + Sync>) -> parser::Brc721Parser {
    parser::Brc721Parser::new(storage)
}
