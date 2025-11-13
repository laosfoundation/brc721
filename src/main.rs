use anyhow::{Context, Result};
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
    log::info!("🚀 Starting brc721");
    log::info!("🔗 Bitcoin Core RPC URL: {}", ctx.rpc_url);
    log::info!("🔐 Auth: user/pass");
    log::info!("🌐 Network: {}", ctx.network);
    log::info!("📂 Data dir: {}", ctx.data_dir.to_string_lossy());

    init_data_dir(&ctx).context("initializing data dir")?;

    if let Some(cmd) = &cli.cmd {
        cmd.run(&ctx)?;
        return Ok(());
    }

    log::info!("🌐 REST API: http://{}", cli.api_listen);
    log::info!("🧮 Confirmations: {}", ctx.confirmations);
    log::info!("🧮 Batch size: {}", ctx.batch_size);
    if let Some(path) = ctx.log_file.as_deref() {
        log::info!("📝 Log file: {}", path.to_string_lossy());
    }

    let storage = init_storage(&ctx);
    let shutdown = tokio_util::sync::CancellationToken::new();

    let api_addr = cli.api_listen;
    let rest_storage = storage.clone();
    let mut rest_handle = tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            if let Err(e) = rest::serve(api_addr, rest_storage, shutdown).await {
                log::error!("REST server error: {}", e);
            }
        }
    });

    let starting_block = storage
        .load_last()
        .unwrap_or_default()
        .map(|last| last.height + 1)
        .unwrap_or(ctx.start);
    let scanner = init_scanner(&ctx, starting_block);
    let parser = parser::Parser {};
    let core = core::Core::new(storage.clone(), scanner, parser);
    let shutdown_core = shutdown.clone();
    let mut core_handle = tokio::task::spawn_blocking(move || {
        let mut core = core;
        core.run(shutdown_core);
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            log::info!("🧨 Ctrl-C received, shutting down");
        }
        _ = &mut rest_handle => {},
        _ = &mut core_handle => {},

    }
    shutdown.cancel();

    let _ = rest_handle.await;
    let _ = core_handle.await;

    log::info!("✅ Shutdown complete");
    Ok(())
}

fn init_data_dir(ctx: &context::Context) -> Result<()> {
    let data_dir = std::path::PathBuf::from(&ctx.data_dir);
    std::fs::create_dir_all(&data_dir)?;
    Ok(())
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
