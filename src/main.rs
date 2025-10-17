use bitcoincore_rpc::{Auth, Client};
use std::sync::Arc;
mod cli;
mod core;
mod parser;
mod scanner;
mod storage;
mod types;
mod wallet;

fn main() {
    let cli = cli::parse();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if let Some(cmd) = cli.cmd.clone() {
        match cmd {
            cli::Command::Wallet { cmd: wcmd } => match wcmd {
                cli::WalletCmd::Init { network, mnemonic, passphrase } => {
                    let net = wallet::parse_network(Some(network));
                    let _ = std::fs::create_dir_all(&cli.data_dir);
                    match wallet::init_wallet(&cli.data_dir, net, mnemonic, passphrase) {
                        Ok(res) => {
                            if res.created {
                                if let Some(m) = res.mnemonic {
                                    println!("initialized wallet db={} mnemonic=\"{}\"", res.db_path.display(), m);
                                } else {
                                    println!("initialized wallet db={}", res.db_path.display());
                                }
                            } else {
                                println!("wallet already initialized db={}", res.db_path.display());
                            }
                        }
                        Err(e) => {
                            eprintln!("wallet init error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    return;
                }
                cli::WalletCmd::Address { network } => {
                    let net = wallet::parse_network(Some(network));
                    let _ = std::fs::create_dir_all(&cli.data_dir);
                    match wallet::next_address(&cli.data_dir, net) {
                        Ok(addr) => {
                            println!("{}", addr);
                        }
                        Err(e) => {
                            eprintln!("wallet address error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    return;
                }
            },
        }
    }

    log::info!("🚀 Starting brc721");
    log::info!("🔗 RPC URL: {}", cli.rpc_url);
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
