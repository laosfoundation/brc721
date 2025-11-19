use crate::{cli, context, core, parser, rest, scanner, storage};
use anyhow::{Context as AnyhowContext, Result};
use bitcoincore_rpc::Client;
use std::path::Path;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// The main application state.
/// decoupled from CLI parsing to allow for easier testing.
pub struct App {
    config: context::Context,
    storage: Arc<dyn storage::Storage + Send + Sync>,
    shutdown: CancellationToken,
}

impl App {
    /// Create a new App instance.
    /// Dependencies are injected here, making it easy to swap Storage for mocks.
    fn new(config: context::Context, storage: Arc<dyn storage::Storage + Send + Sync>) -> Self {
        Self {
            config,
            storage,
            shutdown: CancellationToken::new(),
        }
    }

    /// Factory method to build the App from CLI arguments.
    /// Handles the "dirty" work of side-effects like logging init and filesystem creation.
    pub fn from_cli() -> Result<(Self, cli::Cli)> {
        let cli = crate::cli::parse();
        let ctx = context::Context::from_cli(&cli);

        // Side-effect: Initialize Logging
        crate::tracing::init(ctx.log_file.as_deref().map(Path::new));
        log_startup_info(&ctx);

        // Side-effect: Initialize Storage
        let storage = init_storage(&ctx.data_dir, ctx.reset)?;

        Ok((Self::new(ctx, storage), cli))
    }

    /// Main entry point for the Daemon.
    pub async fn run_daemon(&self) -> Result<()> {
        self.log_runtime_config();

        // 1. Spawn Tasks
        let mut rest_handle = self.spawn_rest_server();
        let mut core_handle = self.spawn_core_indexer()?;

        // 2. Wait for Signal or Error
        self.wait_for_shutdown(&mut rest_handle, &mut core_handle)
            .await
    }

    // --- Helper Methods ---

    fn spawn_rest_server(&self) -> JoinHandle<()> {
        let addr = self.config.api_listen;
        let store = self.storage.clone();
        let token = self.shutdown.clone();

        tokio::spawn(async move {
            if let Err(e) = rest::serve(addr, store, token).await {
                log::error!("REST server failed: {:#}", e);
            }
        })
    }

    fn spawn_core_indexer(&self) -> Result<JoinHandle<()>> {
        // Prepare dependencies
        let start_block = self.determine_start_block()?;
        let scanner = self.create_scanner(start_block)?;
        let parser = parser::Brc721Parser::new(self.storage.clone());

        // Clone for thread
        let storage = self.storage.clone();
        let token = self.shutdown.clone();

        // Spawn blocking because Bitcoin RPC is synchronous
        let handle = tokio::task::spawn_blocking(move || {
            let mut core = core::Core::new(storage, scanner, parser);
            if let Err(e) = core.run(token) {
                log::error!("Core indexer failed: {:#}", e);
            }
        });

        Ok(handle)
    }

    async fn wait_for_shutdown(
        &self,
        rest_task: &mut JoinHandle<()>,
        core_task: &mut JoinHandle<()>,
    ) -> Result<()> {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => log::info!("🧨 Ctrl-C received, shutting down..."),
            _ = &mut *rest_task => log::error!("REST task exited unexpectedly"),
            _ = &mut *core_task => log::error!("Core task exited unexpectedly"),
        }

        // Broadcast shutdown signal
        self.shutdown.cancel();

        // Await tasks to ensure clean exit
        let _ = rest_task.await;
        let _ = core_task.await;

        log::info!("✅ Shutdown complete");
        Ok(())
    }

    fn determine_start_block(&self) -> Result<u64> {
        let last_processed = self.storage.load_last().context("loading last block")?;
        Ok(last_processed
            .map(|b| b.height + 1)
            .unwrap_or(self.config.start))
    }

    fn create_scanner(&self, start_at: u64) -> Result<scanner::Scanner<Client>> {
        let client = Client::new(self.config.rpc_url.as_ref(), self.config.auth.clone())
            .context("failed to connect to Bitcoin RPC")?;

        Ok(scanner::Scanner::new(client)
            .with_confirmations(self.config.confirmations)
            .with_capacity(self.config.batch_size)
            .with_start_from(start_at))
    }

    fn log_runtime_config(&self) {
        log::info!("🧮 Confirmations: {}", self.config.confirmations);
        log::info!("🧮 Batch size: {}", self.config.batch_size);
        if let Some(path) = self.config.log_file.as_deref() {
            log::info!("📝 Log file: {}", path.to_string_lossy());
        }
    }
}

// --- Standalone Helpers ---

fn log_startup_info(ctx: &context::Context) {
    log::info!("🚀 Starting brc721");
    log::info!("🔗 Bitcoin Core RPC URL: {}", ctx.rpc_url);
    log::info!("🌐 Network: {}", ctx.network);
    log::info!("📂 Data dir: {}", ctx.data_dir.to_string_lossy());
}

fn init_storage(data_dir: &Path, reset: bool) -> Result<Arc<dyn storage::Storage + Send + Sync>> {
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir
        .join("brc721.sqlite")
        .to_string_lossy()
        .into_owned();

    let sqlite = storage::SqliteStorage::new(&db_path);
    if reset {
        sqlite.reset_all().context("resetting storage")?;
    }
    sqlite.init().context("initializing storage")?;

    Ok(Arc::new(sqlite))
}

// --- Entry Point ---

pub async fn run() -> Result<()> {
    let (app, cli) = App::from_cli()?;

    // Handle one-shot commands
    if let Some(cmd) = &cli.cmd {
        return cmd.run(&app.config);
    }

    app.run_daemon().await
}
