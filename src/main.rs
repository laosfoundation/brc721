use bitcoincore_rpc::{Auth, Client};
use std::sync::Arc;
mod cli;
mod core;
mod parser;
mod scanner;
mod storage;

fn main() {
    let cli = cli::parse();

    println!("🚀 Starting brc721");
    println!("🔗 RPC URL: {}", cli.rpc_url);
    println!("🔐 Auth: user/pass");
    println!("🧮 Confirmations: {}", cli.confirmations);
    println!("📂 Data dir: {}", cli.data_dir);

    init_data_dir(&cli);
    let storage = init_storage(&cli);
    let starting_block = storage
        .load_last()
        .unwrap_or_default()
        .map(|last| last.height + 1)
        .unwrap_or(cli.start);
    let scanner = init_scanner(&cli, starting_block);

    let core = core::Core::new(storage.clone(), scanner);
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
