use crate::{cli, context, core, parser, rest, scanner, storage};
use anyhow::{Context as AnyhowContext, Result};
use bitcoincore_rpc::Client;
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

        init_data_dir(&ctx).context("initializing data dir")?;
        let storage = init_storage(&ctx)?;

        Ok((Self { ctx, storage }, cli))
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        CancellationToken::new()
    }

    pub fn starting_block(&self) -> u64 {
        self.storage
            .load_last()
            .unwrap_or_default()
            .map(|last| last.height + 1)
            .unwrap_or(self.ctx.start)
    }

    pub fn build_scanner(&self, start_block: u64) -> Result<scanner::Scanner<Client>> {
        let client = Client::new(self.ctx.rpc_url.as_ref(), self.ctx.auth.clone())
            .context("failed to create RPC client")?;
        Ok(scanner::Scanner::new(client)
            .with_confirmations(self.ctx.confirmations)
            .with_capacity(self.ctx.batch_size)
            .with_start_from(start_block))
    }

    pub fn build_parser(&self) -> parser::Brc721Parser {
        parser::Brc721Parser::new(self.storage.clone())
    }
}

fn init_data_dir(ctx: &context::Context) -> Result<()> {
    let data_dir = std::path::PathBuf::from(&ctx.data_dir);
    std::fs::create_dir_all(&data_dir)?;
    Ok(())
}

fn init_storage(ctx: &context::Context) -> Result<Arc<dyn storage::Storage + Send + Sync>> {
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

pub async fn run_daemon(app: App, cli: cli::Cli) -> Result<()> {
    log::info!("🌐 REST API: http://{}", cli.api_listen);
    log::info!("🧮 Confirmations: {}", app.ctx.confirmations);
    log::info!("🧮 Batch size: {}", app.ctx.batch_size);
    if let Some(path) = app.ctx.log_file.as_deref() {
        log::info!("📝 Log file: {}", path.to_string_lossy());
    }

    let shutdown = app.shutdown_token();

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
    let starting_block = app.starting_block();
    let scanner = app.build_scanner(starting_block)?;
    let parser = app.build_parser();
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

    let _ = rest_handle.await;
    let _ = core_handle.await;

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
