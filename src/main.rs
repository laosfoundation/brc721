use bitcoincore_rpc::{Auth, Client};
use std::sync::Arc;
mod cli;
mod core;
mod parser;
mod scanner;
mod storage;
mod types;

fn main() {
    let cli = cli::parse();

    init_tracing(&cli);

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
        if let Some(parent) = Path::new(&cli.log_file).parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&cli.log_file)
            .ok();
        if let Some(file) = file {
            let (writer, guard) = tracing_appender::non_blocking(file);
            std::mem::forget(guard);
            Some(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(writer),
            )
        } else {
            None
        }
    };

    let fmt_stderr = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_writer(std::io::stderr);

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_stderr);
    if let Some(layer) = file_layer {
        registry.with(layer).init();
    } else {
        registry.init();
    }
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
