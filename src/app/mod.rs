mod wiring;

use crate::{cli, context, core, rest, storage};
use anyhow::{Context as AnyhowContext, Result};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct App {
    pub ctx: context::Context,
    pub storage: Arc<dyn storage::Storage + Send + Sync>,
}

impl App {
    pub fn from_cli() -> Result<(Self, cli::Cli)> {
        let cli = crate::cli::parse();
        let ctx = context::Context::from_cli(&cli);

        crate::tracing::init(ctx.log_file.as_deref().map(Path::new));
        log::info!("🚀 Starting brc721");
        log::info!("🔗 Bitcoin Core RPC URL: {}", ctx.rpc_url);
        log::info!("🔐 Auth: user/pass");
        log::info!("🌐 Network: {}", ctx.network);
        log::info!("📂 Data dir: {}", ctx.data_dir.to_string_lossy());

        wiring::init_data_dir(&ctx).context("initializing data dir")?;
        let storage = wiring::init_storage(&ctx)?;

        Ok((Self { ctx, storage }, cli))
    }

    pub fn starting_block(&self) -> Result<u64> {
        let last = self
            .storage
            .load_last()
            .context("loading last processed block")?;

        Ok(match last {
            Some(last) => last.height + 1,
            None => self.ctx.start,
        })
    }
}

pub async fn run_daemon(app: App, cli: cli::Cli) -> Result<()> {
    log::info!("🌐 REST API: http://{}", cli.api_listen);
    log::info!("🧮 Confirmations: {}", app.ctx.confirmations);
    log::info!("🧮 Batch size: {}", app.ctx.batch_size);
    if let Some(path) = app.ctx.log_file.as_deref() {
        log::info!("📝 Log file: {}", path.to_string_lossy());
    }

    let shutdown = CancellationToken::new();

    // REST
    let api_addr = cli.api_listen;
    let rest_storage = app.storage.clone();
    let rest_shutdown = shutdown.clone();

    let mut rest_handle = tokio::spawn(async move {
        if let Err(e) = rest::serve(api_addr, rest_storage, rest_shutdown).await {
            log::error!("REST server error: {}", e);
        }
    });

    // Core
    let starting_block = app.starting_block()?;
    let scanner = wiring::build_scanner(app.ctx, starting_block)?;
    let parser = wiring::build_parser(app.storage.clone());
    let core_shutdown = shutdown.clone();
    let storage = app.storage.clone();

    let mut core_handle = tokio::task::spawn_blocking(move || {
        let mut core = core::Core::new(storage, scanner, parser);
        if let Err(e) = core.run(core_shutdown) {
            log::error!("Core error: {}", e);
        }
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            log::info!("🧨 Ctrl-C received, shutting down");
        }
        _ = &mut rest_handle => {},
        _ = &mut core_handle => {},
    }

    shutdown.cancel();
    let rest_result = rest_handle.await;
    let core_result = core_handle.await;

    let mut fatal_error: Option<anyhow::Error> = None;

    if let Err(e) = rest_result {
        log::error!("REST server error: {}", e);
        fatal_error = Some(e.into());
    }
    if let Err(e) = core_result {
        log::error!("Core error: {}", e);
        fatal_error.get_or_insert(e.into());
    }

    if let Some(e) = fatal_error {
        return Err(e);
    }

    log::info!("✅ Shutdown complete");
    Ok(())
}

pub async fn run() -> Result<()> {
    let (app, cli) = App::from_cli()?;

    if let Some(cmd) = &cli.cmd {
        // one-shot command mode
        cmd.run(&app.ctx)?;
        return Ok(());
    }

    run_daemon(app, cli).await
}
