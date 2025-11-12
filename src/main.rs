use anyhow::Result;
use bitcoincore_rpc::Client;
use std::path::Path;
use std::sync::Arc;

mod cli;
mod commands;
mod context;
mod core;
mod parser;
mod rest;
mod scanner;
mod storage;
mod tracing;
pub mod types;
mod wallet;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::parse();
    let ctx = context::Context::from_cli(&cli);

    tracing::init(ctx.log_file.as_deref().map(Path::new));

    init_data_dir(&ctx);

    if let Some(cmd) = &cli.cmd {
        cmd.run(&ctx)?;
        return Ok(());
    }

    log::info!("🚀 Starting brc721");
    log::info!("🔗 Bitcoin Core RPC URL: {}", ctx.rpc_url);
    log::info!("🌐 Network: {}", ctx.network);
    log::info!("🌐 REST API: http://{}", cli.api_listen);
    log::info!("🔐 Auth: user/pass");
    log::info!("📂 Data dir: {}", ctx.data_dir.to_string_lossy());
    log::info!("🧮 Confirmations: {}", ctx.confirmations);
    log::info!("🧮 Batch size: {}", ctx.batch_size);
    if let Some(path) = ctx.log_file.as_deref() {
        log::info!("📝 Log file: {}", path.to_string_lossy());
    }

    let storage = init_storage(&ctx);
    let starting_block = storage
        .load_last()
        .unwrap_or_default()
        .map(|last| last.height + 1)
        .unwrap_or(ctx.start);
    let scanner = init_scanner(&ctx, starting_block);
    let parser = parser::Parser {};

    let api_addr = cli.api_listen;
    let rest_storage = storage.clone();
    tokio::spawn(async move {
        if let Err(e) = rest::serve(api_addr, rest_storage).await {
            log::error!("REST server error: {}", e);
        }
    });

    let core = core::Core::new(storage.clone(), scanner, parser);
    core.run();
}

fn init_data_dir(ctx: &context::Context) {
    let data_dir = std::path::PathBuf::from(&ctx.data_dir);
    let _ = std::fs::create_dir_all(&data_dir);
}

fn init_storage(ctx: &context::Context) -> Arc<dyn storage::Storage + Send + Sync> {
    let data_dir = std::path::PathBuf::from(&ctx.data_dir);
    let db_path = data_dir
        .join("brc721.sqlite")
        .to_string_lossy()
        .into_owned();
    let sqlite = storage::SqliteStorage::new(&db_path);
    if ctx.reset {
        let _ = sqlite.reset_all();
    }
    let _ = sqlite.init();
    Arc::new(sqlite)
}

fn init_scanner(ctx: &context::Context, start_block: u64) -> scanner::Scanner<Client> {
    let client =
        Client::new(ctx.rpc_url.as_ref(), ctx.auth.clone()).expect("failed to create RPC client");
    scanner::Scanner::new(client)
        .with_confirmations(ctx.confirmations)
        .with_capacity(ctx.batch_size)
        .with_start_from(start_block)
}

#[cfg(test)]
mod integration_tests;
