use bitcoincore_rpc::{Auth, Client};
use std::sync::Arc;
mod cli;
mod core;
mod p2p;
mod parser;
mod scanner;
mod storage;
mod types;

fn main() {
    let cli = cli::parse();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("🚀 Starting brc721");
    log::info!("🔗 Network: {}", cli.network);
    log::info!("🔗 RPC URL: {}", cli.rpc_url);
    log::info!("🔗 P2P peer: {}", cli.p2p_peer);
    log::info!("🔐 Auth: user/pass");
    log::info!("🧮 Confirmations: {}", cli.confirmations);
    log::info!("📂 Data dir: {}", cli.data_dir);

    init_data_dir(&cli);
    let storage = init_storage(&cli);
    let starting_block = storage
        .load_last()
        .unwrap_or_default()
        .map(|last| last.height + 1)
        .unwrap_or(cli.start);
    let scanner = init_scanner(&cli, starting_block);
    let parser = parser::Parser {};

    let core = core::Core::new(storage.clone(), scanner, parser);
    core.run();
}

fn init_data_dir(cli: &cli::Cli) {
    let data_dir = std::path::PathBuf::from(&cli.data_dir);
    let _ = std::fs::create_dir_all(&data_dir);
}

fn init_storage(cli: &cli::Cli) -> Arc<dyn storage::Storage + Send + Sync> {
    let data_dir = std::path::PathBuf::from(&cli.data_dir);
    let db_path = data_dir
        .join("brc721.sqlite")
        .to_string_lossy()
        .into_owned();
    let sqlite = storage::SqliteStorage::new(&db_path);
    if cli.reset {
        let _ = sqlite.reset_all();
    }
    let _ = sqlite.init();
    Arc::new(sqlite)
}

fn init_scanner(cli: &cli::Cli, start_block: u64) -> Box<dyn scanner::BlockScanner + Send> {
    let auth = match (&cli.rpc_user, &cli.rpc_pass) {
        (Some(user), Some(pass)) => Auth::UserPass(user.clone(), pass.clone()),
        _ => Auth::None,
    };

    let client = Client::new(&cli.rpc_url, auth).expect("failed to create RPC client");
    let magic = p2p::magic_from_network_name(&cli.network);
    let peer = if !cli.p2p_peer.is_empty() {
        Some(cli.p2p_peer.clone())
    } else {
        derive_p2p_addr_from_rpc_url(&cli.rpc_url, &cli.network)
    };
    if let Some(addr) = peer {
        match scanner::P2PFetcher::connect(&addr, magic) {
            Ok(fetcher) => {
                let sc = scanner::P2pScanner::new(client, fetcher)
                    .with_confirmations(cli.confirmations)
                    .with_capacity(cli.batch_size)
                    .with_start_from(start_block);
                log::info!("P2P enabled: {} ({})", addr, cli.network);
                return Box::new(sc);
            }
            Err(e) => {
                log::warn!("P2P connect failed: {} - using RPC only", e);
            }
        }
    }
    let sc = scanner::RpcScanner::new(client)
        .with_confirmations(cli.confirmations)
        .with_capacity(cli.batch_size)
        .with_start_from(start_block);
    Box::new(sc)
}

fn derive_p2p_addr_from_rpc_url(rpc_url: &str, network: &str) -> Option<String> {
    let s = rpc_url.trim();
    let s = s
        .strip_prefix("http://")
        .or_else(|| s.strip_prefix("https://"))
        .unwrap_or(s);
    let host_port = s.split('/').next().unwrap_or("");
    let host = if let Some((h, _p)) = host_port.rsplit_once('@') {
        h
    } else {
        host_port
    };
    let host = if let Some((h, _)) = host.split_once(':') {
        h
    } else {
        host
    };
    if host.is_empty() {
        return None;
    }
    let port = p2p::default_p2p_port_for_network(network);
    Some(format!("{}:{}", host, port))
}
