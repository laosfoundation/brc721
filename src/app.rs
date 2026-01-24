use crate::{
    bitcoin_rpc::BitcoinRpc,
    context, core, parser, rest,
    scanner::{self},
    storage::{self, Storage},
};
use anyhow::{anyhow, Context as AnyhowContext, Result};
use bitcoincore_rpc::Client;
use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct App {
    config: context::Context,
    shutdown: CancellationToken,
    db_path: PathBuf,
}

impl App {
    pub fn new(config: context::Context) -> Result<Self> {
        let db_path = config.data_dir.join("brc721.sqlite");
        setup_storage(&config.data_dir, config.reset)?;
        Ok(Self {
            config,
            shutdown: CancellationToken::new(),
            db_path,
        })
    }

    /// Main entry point for the Daemon.
    pub async fn run_daemon<C: BitcoinRpc + Send + Sync + 'static>(
        &mut self,
        client: C,
    ) -> Result<()> {
        // 1. Spawn Tasks
        let mut rest_handle = self.spawn_rest_server();
        let mut core_handle = self.spawn_core_indexer(client)?;

        // 2. Wait for Signal or Error
        self.wait_for_shutdown(&mut rest_handle, &mut core_handle)
            .await
    }

    fn spawn_rest_server(&self) -> JoinHandle<()> {
        let addr = self.config.api_listen;
        let storage = storage::SqliteStorage::new(self.db_path.clone());
        let token = self.shutdown.clone();

        let network = self.config.network;
        tokio::spawn(async move {
            if let Err(e) = rest::serve(addr, storage, network, token).await {
                log::error!("REST server failed: {:#}", e);
            }
        })
    }

    fn spawn_core_indexer<C: BitcoinRpc + Send + Sync + 'static>(
        &mut self,
        client: C,
    ) -> Result<JoinHandle<Result<()>>> {
        let storage = storage::SqliteStorage::new(self.db_path.clone());
        let start_block = determine_start_block(&storage, self.config.start)?;

        let scanner = scanner::Scanner::new(client)
            .with_confirmations(self.config.confirmations)
            .with_capacity(self.config.batch_size)
            .with_start_from(start_block);
        let parser = parser::Brc721Parser::new();
        let token = self.shutdown.clone();

        // Spawn blocking because Bitcoin RPC is synchronous
        let handle = tokio::task::spawn_blocking(move || -> Result<()> {
            let mut core = core::Core::new(scanner, storage, parser);
            core.run(token)?;
            Ok(())
        });

        Ok(handle)
    }

    async fn wait_for_shutdown(
        &self,
        rest_task: &mut JoinHandle<()>,
        core_task: &mut JoinHandle<Result<()>>,
    ) -> Result<()> {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => log::info!("🧨 Ctrl-C received, shutting down..."),
            _ = &mut *rest_task => {
                self.shutdown.cancel();
                log::error!("REST task exited unexpectedly");
                if !core_task.is_finished() {
                    let _ = core_task.await;
                }
                return Err(anyhow!("REST task exited unexpectedly"));
            },
            res = &mut *core_task => {
                self.shutdown.cancel();
                if !rest_task.is_finished() {
                    let _ = rest_task.await;
                }
                match res {
                    Ok(Ok(())) => return Err(anyhow!("Core task exited unexpectedly")),
                    Ok(Err(e)) => return Err(e),
                    Err(join_err) => return Err(anyhow!(join_err).context("core task join error")),
                }
            },
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
}

// --- Standalone Helpers ---
fn setup_storage(data_dir: &Path, reset: bool) -> Result<()> {
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

    Ok(())
}

fn determine_start_block<S: Storage>(storage: &S, default: u64) -> Result<u64> {
    let last_processed = storage.load_last().context("loading last block")?;
    Ok(last_processed.map(|b| b.height + 1).unwrap_or(default))
}

// --- Entry Point ---
pub async fn run() -> Result<()> {
    crate::tracing::init(None);
    log::info!("🚀 Starting brc721");

    let cli = crate::cli::parse();
    let ctx = context::Context::from_cli(&cli);

    if let Some(path) = ctx.log_file.as_deref() {
        log::info!("📝 Log file: {}", path.to_string_lossy());
        crate::tracing::init(ctx.log_file.as_deref().map(Path::new));
    }

    log::info!("🔗 Bitcoin Core RPC URL: {}", ctx.rpc_url);
    log::info!("🌐 Network: {}", ctx.network);
    log::info!("📂 Data dir: {}", ctx.data_dir.to_string_lossy());

    // Handle one-shot commands
    if let Some(cmd) = &cli.cmd {
        return cmd.run(&ctx);
    }

    let client = Client::new(ctx.rpc_url.as_ref(), ctx.auth.clone())
        .context("failed to connect to Bitcoin RPC")?;

    App::new(ctx)?.run_daemon(client).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::traits::{
        Block, Collection, CollectionKey, OwnershipRange, OwnershipRangeWithGroup, OwnershipUtxo,
        OwnershipUtxoSave, StorageRead, StorageTx, StorageWrite,
    };
    use bitcoin::hashes::Hash;
    use bitcoincore_rpc::Error as RpcError;
    use ethereum_types::H160;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Clone)]
    struct DummyStorage {
        last: Arc<Mutex<Option<Block>>>,
    }

    impl DummyStorage {
        fn new() -> Self {
            Self {
                last: Arc::new(Mutex::new(None)),
            }
        }

        fn with_last(self, height: u64) -> Self {
            *self.last.lock().unwrap() = Some(Block {
                height,
                hash: "hash".to_string(),
            });
            self
        }
    }

    impl StorageRead for DummyStorage {
        fn load_last(&self) -> Result<Option<Block>> {
            Ok(self.last.lock().unwrap().clone())
        }

        fn load_collection(&self, _id: &CollectionKey) -> Result<Option<Collection>> {
            Ok(None)
        }

        fn list_collections(&self) -> Result<Vec<Collection>> {
            Ok(Vec::new())
        }

        fn list_unspent_ownership_utxos_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> Result<Vec<OwnershipUtxo>> {
            Ok(vec![])
        }

        fn list_unspent_ownership_ranges_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> Result<Vec<OwnershipRangeWithGroup>> {
            Ok(vec![])
        }

        fn list_ownership_ranges(&self, _utxo: &OwnershipUtxo) -> Result<Vec<OwnershipRange>> {
            Ok(vec![])
        }

        fn find_unspent_ownership_utxo_for_slot(
            &self,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot: u128,
        ) -> Result<Option<OwnershipUtxo>> {
            Ok(None)
        }

        fn list_unspent_ownership_utxos_by_owner(
            &self,
            _owner_h160: H160,
        ) -> Result<Vec<OwnershipUtxo>> {
            Ok(vec![])
        }
    }

    impl StorageWrite for DummyStorage {
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

        fn save_ownership_utxo(&self, _utxo: OwnershipUtxoSave<'_>) -> Result<()> {
            Ok(())
        }

        fn save_ownership_range(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot_start: u128,
            _slot_end: u128,
        ) -> Result<()> {
            Ok(())
        }

        fn mark_ownership_utxo_spent(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _spent_txid: &str,
            _spent_height: u64,
            _spent_tx_index: u32,
        ) -> Result<()> {
            Ok(())
        }
    }

    impl StorageTx for DummyStorage {
        fn commit(self) -> Result<()> {
            Ok(())
        }
    }

    impl storage::Storage for DummyStorage {
        type Tx = DummyStorage;

        fn begin_tx(&self) -> Result<Self::Tx> {
            Ok(self.clone())
        }
    }

    #[derive(Clone)]
    struct DummyRpc;

    impl crate::bitcoin_rpc::BitcoinRpc for DummyRpc {
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
        fn get_raw_transaction(
            &self,
            _txid: &bitcoin::Txid,
        ) -> Result<bitcoin::Transaction, RpcError> {
            unimplemented!()
        }
        fn wait_for_new_block(&self, _timeout: u64) -> Result<(), RpcError> {
            Ok(())
        }
    }

    fn make_app_with_storage(_storage: DummyStorage) -> (App, DummyRpc, TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = context::Context {
            rpc_url: url::Url::parse("http://localhost:8332").unwrap(),
            auth: bitcoincore_rpc::Auth::None,
            network: bitcoin::Network::Regtest,
            start: 0,
            confirmations: 1,
            batch_size: 1,
            data_dir: temp_dir.path().to_path_buf(),
            reset: false,
            log_file: None,
            api_listen: "127.0.0.1:3000".parse().unwrap(),
        };
        let rpc = DummyRpc;
        (App::new(config).unwrap(), rpc, temp_dir)
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

        let start = determine_start_block(&storage, config.start).unwrap();
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

        let start = determine_start_block(&storage, config.start).unwrap();
        assert_eq!(start, 101);
    }

    #[tokio::test]
    async fn spawn_core_indexer_creates_thread() {
        let storage = DummyStorage::new();
        let (mut app, rpc, _temp) = make_app_with_storage(storage);

        let res = app.spawn_core_indexer(rpc);
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn wait_for_shutdown_exits_when_task_finishes() {
        let storage = DummyStorage::new();
        let (app, _rpc, _temp) = make_app_with_storage(storage);
        let token = app.shutdown.clone();

        // Create dummy tasks
        // 1. Simulates a task that fails/exits, triggering the shutdown
        let mut rest_task = tokio::spawn(async {
            // Exit immediately
        });

        // 2. Simulates a task that runs until shutdown is signaled
        let mut core_task = tokio::spawn(async move {
            token.cancelled().await;
            Ok::<(), anyhow::Error>(())
        });

        // Should return:
        // 1. rest_task finishes -> select! matches
        // 2. app.shutdown.cancel() is called
        // 3. core_task sees cancellation and finishes
        // 4. wait_for_shutdown awaits core_task and returns
        let res = app.wait_for_shutdown(&mut rest_task, &mut core_task).await;
        assert!(res.is_err());

        // Verify shutdown signal was sent
        assert!(app.shutdown.is_cancelled());
    }

    #[tokio::test]
    async fn spawn_rest_server_starts_and_serves_health_check() {
        let storage = DummyStorage::new();
        let (mut app, _rpc, _temp) = make_app_with_storage(storage);
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .map(|addr| addr.port())
            .expect("pick open port");
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
        assert!(response.contains("uptimeSecs"));

        // Cleanup
        app.shutdown.cancel();
        let _ = handle.await;
    }
}
