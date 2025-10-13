use std::sync::Arc;

use bitcoincore_rpc::{Auth, Client};
mod cli;
mod core;
mod parser;
mod scanner;
mod storage;
use storage::Storage;

fn main() {
    let cli = cli::parse();

    let auth_mode = match (&cli.rpc_user, &cli.rpc_pass) {
        (Some(_), Some(_)) => "user/pass",
        _ => "none",
    };

    println!("🚀 Starting brc721");
    println!("🔗 RPC URL: {}", cli.rpc_url);
    println!("🔐 Auth: {}", auth_mode);
    println!("🛠️ Debug: {}", if cli.debug { "on" } else { "off" });
    println!("🧮 Confirmations: {}", cli.confirmations);
    println!("📂 Data dir: {}", cli.data_dir);

    init_data_dir(&cli);
    let storage_arc = init_storage(&cli);
    let starting_block = storage_arc
        .load_last()
        .unwrap_or_default()
        .map(|last| last.height + 1)
        .unwrap_or_default();
    let scanner = init_scanner(&cli, starting_block);

    let core = core::Core::new(storage_arc.clone(), scanner, cli.debug, cli.batch_size);
    core.run();
}

fn init_data_dir(cli: &cli::Cli) {
    let data_dir = std::path::PathBuf::from(&cli.data_dir);
    let _ = std::fs::create_dir_all(&data_dir);
}

fn init_storage(cli: &cli::Cli) -> Arc<dyn Storage + Send + Sync> {
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

fn init_scanner(cli: &cli::Cli, start_block: u64) -> scanner::Scanner<Client> {
    let auth = match (&cli.rpc_user, &cli.rpc_pass) {
        (Some(user), Some(pass)) => Auth::UserPass(user.clone(), pass.clone()),
        _ => Auth::None,
    };

    let client = Client::new(&cli.rpc_url, auth).expect("failed to create RPC client");
    scanner::Scanner::new(client)
        .with_confirmations(cli.confirmations)
        .with_capacity(cli.batch_size)
        .with_start_from(start_block)
}
