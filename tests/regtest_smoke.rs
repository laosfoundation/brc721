use bitcoincore_rpc::{Auth, Client, RpcApi};
use std::env;

fn rpc_client() -> Client {
    dotenvy::from_filename(".env.test").ok();
    dotenvy::dotenv().ok();
    let url = env::var("BITCOIN_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:18443".to_string());
    let auth = match (
        env::var("BITCOIN_RPC_USER").ok(),
        env::var("BITCOIN_RPC_PASS").ok(),
        env::var("BITCOIN_RPC_COOKIE").ok(),
    ) {
        (Some(u), Some(p), _) => Auth::UserPass(u, p),
        (_, _, Some(c)) => Auth::CookieFile(c.into()),
        _ => Auth::None,
    };
    Client::new(&url, auth).expect("failed to create RPC client")
}

#[test]
fn can_read_tip_and_header() {
    let client = rpc_client();
    let tip = client.get_block_count().expect("get_block_count");
    let hash = client.get_block_hash(tip).expect("get_block_hash");
    let header = client
        .get_block_header_info(&hash)
        .expect("get_block_header_info");
    assert!(header.time > 0);
}
