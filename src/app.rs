use crate::{
    cli, context, core, parser, rest,
    scanner::{self, BitcoinRpc},
    storage,
};
use anyhow::{Context as AnyhowContext, Result};
use bitcoincore_rpc::Client;
use std::path::Path;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// The main application state.
/// decoupled from CLI parsing to allow for easier testing.
pub struct App<C: BitcoinRpc = Client> {
    config: context::Context,
    storage: Arc<dyn storage::Storage + Send + Sync>,
    shutdown: CancellationToken,
    scanner: Option<scanner::Scanner<C>>,
}

impl App {
    /// Factory method to build the App from CLI arguments.
    /// Handles the "dirty" work of side-effects like logging init and filesystem creation.
    pub fn from_cli() -> Result<(App<Client>, cli::Cli)> {
        let cli = crate::cli::parse();

        // Configure logging file after CLI is parsed
        crate::tracing::set_log_file(cli.log_file.as_deref().map(Path::new));

        let ctx = context::Context::from_cli(&cli);
        log_startup_info(&ctx);

        // Side-effect: Initialize Storage
        let storage = init_storage(&ctx.data_dir, ctx.reset)?;

        // Prepare Scanner Dependencies
        let start_block = determine_start_block(storage.as_ref(), &ctx)?;
        let client = Client::new(ctx.rpc_url.as_ref(), ctx.auth.clone())
            .context("failed to connect to Bitcoin RPC")?;

        let scanner = scanner::Scanner::new(client)
            .with_confirmations(ctx.confirmations)
            .with_capacity(ctx.batch_size)
            .with_start_from(start_block);

        Ok((App::new(ctx, storage, scanner), cli))
    }
}

impl<C: BitcoinRpc + Send + Sync + 'static> App<C> {
    /// Create a new App instance.
    /// Dependencies are injected here, making it easy to swap Storage for mocks.
    fn new(
        config: context::Context,
        storage: Arc<dyn storage::Storage + Send + Sync>,
        scanner: scanner::Scanner<C>,
    ) -> Self {
        Self {
            config,
            storage,
            shutdown: CancellationToken::new(),
            scanner: Some(scanner),
        }
    }

    /// Main entry point for the Daemon.
    pub async fn run_daemon(&mut self) -> Result<()> {
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

    fn spawn_core_indexer(&mut self) -> Result<JoinHandle<()>> {
        // Take the scanner out of the Option
        let scanner = self.scanner.take().context("Scanner already consumed")?;
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
        // Check is_finished() to avoid polling a completed JoinHandle (which panics)
        if !rest_task.is_finished() {
            let _ = rest_task.await;
        }
        if !core_task.is_finished() {
            let _ = core_task.await;
        }

        log::info!("✅ Shutdown complete");
        Ok(())
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

fn determine_start_block(
    storage: &(dyn storage::Storage + Send + Sync),
    config: &context::Context,
) -> Result<u64> {
    let last_processed = storage.load_last().context("loading last block")?;
    Ok(last_processed.map(|b| b.height + 1).unwrap_or(config.start))
}

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
    let (mut app, cli) = App::from_cli()?;

    // Handle one-shot commands
    if let Some(cmd) = &cli.cmd {
        return cmd.run(&app.config);
    }

    app.run_daemon().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::traits::{Block, CollectionKey};
    use bitcoin::hashes::Hash;
    use bitcoincore_rpc::Error as RpcError;
    use ethereum_types::H160;
    use std::sync::Mutex;

    struct DummyStorage {
        last: Mutex<Option<Block>>,
    }

    impl DummyStorage {
        fn new() -> Self {
            Self {
                last: Mutex::new(None),
            }
        }

        fn with_last(mut self, height: u64) -> Self {
            self.last = Mutex::new(Some(Block {
                height,
                hash: "hash".to_string(),
            }));
            self
        }
    }

    impl storage::Storage for DummyStorage {
        fn load_last(&self) -> Result<Option<Block>> {
            Ok(self.last.lock().unwrap().clone())
        }

        fn save_last(&self, height: u64, hash: &str) -> Result<()> {
            *self.last.lock().unwrap() = Some(Block {
                height,
                hash: hash.to_string(),
            });
            Ok(())
        }

        fn save_collection(
            &self,
            _key: CollectionKey,
            _evm_collection_address: H160,
            _rebaseable: bool,
        ) -> Result<()> {
            Ok(())
        }

        fn list_collections(&self) -> Result<Vec<(CollectionKey, String, bool)>> {
            Ok(Vec::new())
        }
    }

    #[derive(Clone)]
    struct DummyRpc;

    impl scanner::BitcoinRpc for DummyRpc {
        fn get_block_count(&self) -> Result<u64, RpcError> {
            Ok(100)
        }
        fn get_block_hash(&self, _height: u64) -> Result<bitcoin::BlockHash, RpcError> {
            Ok(bitcoin::BlockHash::all_zeros())
        }
        fn get_block(&self, _hash: &bitcoin::BlockHash) -> Result<bitcoin::Block, RpcError> {
            Err(RpcError::JsonRpc(bitcoincore_rpc::jsonrpc::Error::Rpc(
                bitcoincore_rpc::jsonrpc::error::RpcError {
                    code: -1,
                    message: "not implemented".into(),
                    data: None,
                },
            )))
        }
        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            Ok(())
        }
    }

    fn make_app_with_storage(storage: DummyStorage) -> App<DummyRpc> {
        let config = context::Context {
            rpc_url: url::Url::parse("http://localhost:8332").unwrap(),
            auth: bitcoincore_rpc::Auth::None,
            network: bitcoin::Network::Regtest,
            start: 0,
            confirmations: 1,
            batch_size: 1,
            data_dir: std::path::PathBuf::from("."),
            reset: false,
            log_file: None,
            api_listen: "127.0.0.1:3000".parse().unwrap(),
        };
        let storage = Arc::new(storage);
        let rpc = DummyRpc;
        let scanner = scanner::Scanner::new(rpc);
        App::new(config, storage, scanner)
    }

    #[test]
    fn determine_start_block_uses_config_when_storage_empty() {
        let storage = DummyStorage::new();
        let config = context::Context {
            rpc_url: url::Url::parse("http://localhost:8332").unwrap(),
            auth: bitcoincore_rpc::Auth::None,
            network: bitcoin::Network::Regtest,
            start: 123,
            confirmations: 1,
            batch_size: 1,
            data_dir: std::path::PathBuf::from("."),
            reset: false,
            log_file: None,
            api_listen: "127.0.0.1:3000".parse().unwrap(),
        };

        let start = determine_start_block(&storage, &config).unwrap();
        assert_eq!(start, 123);
    }

    #[test]
    fn determine_start_block_uses_storage_plus_one_when_present() {
        let storage = DummyStorage::new().with_last(100);
        let config = context::Context {
            rpc_url: url::Url::parse("http://localhost:8332").unwrap(),
            auth: bitcoincore_rpc::Auth::None,
            network: bitcoin::Network::Regtest,
            start: 0,
            confirmations: 1,
            batch_size: 1,
            data_dir: std::path::PathBuf::from("."),
            reset: false,
            log_file: None,
            api_listen: "127.0.0.1:3000".parse().unwrap(),
        };

        let start = determine_start_block(&storage, &config).unwrap();
        assert_eq!(start, 101);
    }

    #[tokio::test]
    async fn spawn_core_indexer_creates_thread() {
        let storage = DummyStorage::new();
        let mut app = make_app_with_storage(storage);

        let res = app.spawn_core_indexer();
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn spawn_core_indexer_fails_on_second_call() {
        let storage = DummyStorage::new();
        let mut app = make_app_with_storage(storage);

        let _ = app.spawn_core_indexer();
        let res = app.spawn_core_indexer();
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), "Scanner already consumed");
    }

    #[tokio::test]
    async fn wait_for_shutdown_exits_when_task_finishes() {
        let storage = DummyStorage::new();
        let app = make_app_with_storage(storage);
        let token = app.shutdown.clone();

        // Create dummy tasks
        // 1. Simulates a task that fails/exits, triggering the shutdown
        let mut rest_task = tokio::spawn(async {
            // Exit immediately
        });

        // 2. Simulates a task that runs until shutdown is signaled
        let mut core_task = tokio::spawn(async move {
            token.cancelled().await;
        });

        // Should return:
        // 1. rest_task finishes -> select! matches
        // 2. app.shutdown.cancel() is called
        // 3. core_task sees cancellation and finishes
        // 4. wait_for_shutdown awaits core_task and returns
        let res = app.wait_for_shutdown(&mut rest_task, &mut core_task).await;
        assert!(res.is_ok());

        // Verify shutdown signal was sent
        assert!(app.shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn spawn_rest_server_starts_and_serves_health_check() {
        let storage = DummyStorage::new();
        let mut app = make_app_with_storage(storage);
        // Use a random-ish port to avoid conflicts
        let port = 34567;
        app.config.api_listen = format!("127.0.0.1:{}", port).parse().unwrap();

        let handle = app.spawn_rest_server();

        // Give it a moment to bind
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Verify task is still running
        assert!(
            !handle.is_finished(),
            "REST server task finished unexpectedly (likely bind failed)"
        );

        // Send HTTP request
        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await;
        assert!(stream.is_ok(), "Failed to connect to REST server");
        let mut stream = stream.unwrap();

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();

        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).await.unwrap();
        let response = String::from_utf8_lossy(&buffer);

        assert!(response.contains("200 OK"));
        // Check JSON content (field name depends on serde rename, checking raw field)
        assert!(response.contains("uptime_secs"));

        // Cleanup
        app.shutdown.cancel();
        let _ = handle.await;
    }
}
