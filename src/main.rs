mod app;
mod bitcoin_rpc;
mod cli;
mod commands;
mod context;
mod core;
mod parser;
mod rest;
mod scanner;
mod storage;
mod tracing;
mod types;
mod wallet;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    crate::tracing::init(None);

    if let Err(e) = app::run().await {
        log::error!("Fatal error: {:#}", e);
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod integration_tests;
