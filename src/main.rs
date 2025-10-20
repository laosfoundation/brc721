use bitcoincore_rpc::{Auth, Client};
use std::path::Path;
use std::sync::Arc;
mod cli;
mod core;
mod parser;
mod scanner;
mod storage;
mod tracing;
mod types;

fn main() {
    let cli = cli::parse();

    tracing::init(Path::new(&cli.log_file));

    log::info!("🚀 Starting brc721");
    log::info!("🔗 RPC URL: {}", cli.rpc_url);
    log::info!("🔐 Auth: user/pass");
    log::info!("🧮 Confirmations: {}", cli.confirmations);
    log::info!("📂 Data dir: {}", cli.data_dir);
    log::info!("🗒️ Log file: {}", cli.log_file);

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

fn init_tracing(cli: &cli::Cli) {
    use tracing_subscriber::prelude::*;
    let _ = tracing_log::LogTracer::init();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let file_layer = {
        use std::fs::OpenOptions;
        use std::path::Path;
        let path = Path::new(&cli.log_file);
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(parent);
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(|file| {
                let (writer, guard) = tracing_appender::non_blocking(file);
                std::mem::forget(guard);
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(writer)
            })
    };

    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
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
