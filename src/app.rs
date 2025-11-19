use crate::{cli, context, core, parser, rest, scanner, storage};
use anyhow::{Context as AnyhowContext, Result};
use bitcoincore_rpc::Client;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct App {
    ctx: context::Context,
    storage: Arc<dyn storage::Storage + Send + Sync>,
    shutdown: CancellationToken,
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

        let storage = init_storage(&ctx)?;

        Ok((
            Self {
                ctx,
                storage,
                shutdown: CancellationToken::new(),
            },
            cli,
        ))
    }

    pub async fn run_daemon(&self, cli: cli::Cli) -> Result<()> {
        log::info!("🌐 REST API: http://{}", cli.api_listen);
        log::info!("🧮 Confirmations: {}", self.ctx.confirmations);
        log::info!("🧮 Batch size: {}", self.ctx.batch_size);
        if let Some(path) = self.ctx.log_file.as_deref() {
            log::info!("📝 Log file: {}", path.to_string_lossy());
        }

        let mut rest_handle = self.spaw_rest_task()?;
        let mut core_handle = self.spaw_core_task()?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::info!("🧨 Ctrl-C received, shutting down");
            }
            _ = &mut rest_handle => {},
            _ = &mut core_handle => {},
        }

        self.shutdown.cancel();
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

    fn spaw_core_task(&self) -> Result<tokio::task::JoinHandle<()>> {
        let starting_block = self.starting_block()?;
        let scanner = self.build_scanner(starting_block)?;
        let parser = self.build_parser();
        let core_shutdown = self.shutdown.clone();
        let storage = self.storage.clone();

        let handler = tokio::task::spawn_blocking(move || {
            let mut core = core::Core::new(storage, scanner, parser);
            if let Err(e) = core.run(core_shutdown) {
                log::error!("Core error: {}", e);
            }
        });

        Ok(handler)
    }

    fn spaw_rest_task(&self) -> Result<tokio::task::JoinHandle<()>> {
        let api_addr = self.ctx.api_listen;
        let rest_storage = self.storage.clone();
        let rest_shutdown = self.shutdown.clone();

        let handler = tokio::spawn(async move {
            if let Err(e) = rest::serve(api_addr, rest_storage, rest_shutdown).await {
                log::error!("REST server error: {}", e);
            }
        });

        Ok(handler)
    }

    fn starting_block(&self) -> Result<u64> {
        let last = self
            .storage
            .load_last()
            .context("loading last processed block")?;

        Ok(match last {
            Some(last) => last.height + 1,
            None => self.ctx.start,
        })
    }

    fn build_scanner(&self, start_block: u64) -> Result<scanner::Scanner<Client>> {
        let client = Client::new(self.ctx.rpc_url.as_ref(), self.ctx.auth.clone())
            .context("failed to create RPC client")?;
        Ok(scanner::Scanner::new(client)
            .with_confirmations(self.ctx.confirmations)
            .with_capacity(self.ctx.batch_size)
            .with_start_from(start_block))
    }

    fn build_parser(&self) -> parser::Brc721Parser {
        parser::Brc721Parser::new(self.storage.clone())
    }
}

pub fn init_storage(ctx: &context::Context) -> Result<Arc<dyn storage::Storage + Send + Sync>> {
    let data_dir = std::path::PathBuf::from(&ctx.data_dir);
    std::fs::create_dir_all(&data_dir)?;

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

pub async fn run() -> Result<()> {
    let (app, cli) = App::from_cli()?;

    if let Some(cmd) = &cli.cmd {
        // one-shot command mode
        cmd.run(&app.ctx)?;
        return Ok(());
    }

    app.run_daemon(cli).await
}
