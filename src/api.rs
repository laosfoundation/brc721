use std::net::SocketAddr;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bitcoincore_rpc::{Auth, Client};
use serde::{Deserialize, Serialize};

use crate::wallet::Wallet;

#[derive(Clone)]
pub struct ApiState {
    pub rpc_url: String,
    pub auth: Auth,
    pub token: Option<String>,
}

#[derive(Deserialize)]
struct InitWalletReq {
    name: String,
}

#[derive(Serialize)]
struct InitWalletResp {
    ok: bool,
    name: String,
}

#[derive(Serialize)]
struct AddressResp {
    address: String,
}

#[derive(Serialize)]
struct BalanceResp {
    balance_btc: f64,
}

#[derive(Deserialize)]
struct CreateCollectionReq {
    wallet: String,
    laos_hex: String,
    #[serde(default)]
    rebaseable: bool,
    #[serde(default)]
    fee_rate: Option<f64>,
}

#[derive(Serialize)]
struct CreateCollectionResp {
    txid: String,
}

fn is_authorized(headers: &HeaderMap, token: &Option<String>) -> bool {
    match token {
        None => true,
        Some(t) => {
            if let Some(v) = headers.get("x-api-key") {
                if v.to_str().ok() == Some(t.as_str()) {
                    return true;
                }
            }
            if let Some(v) = headers.get(axum::http::header::AUTHORIZATION) {
                if let Ok(s) = v.to_str() {
                    if let Some(rest) = s.strip_prefix("Bearer ") {
                        if rest == t {
                            return true;
                        }
                    }
                }
            }
            false
        }
    }
}

async fn wallet_init(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<InitWalletReq>,
) -> impl IntoResponse {
    if !is_authorized(&headers, &state.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let client = match Client::new(&state.rpc_url, state.auth.clone()) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match Wallet::create_wallet(&client, &req.name) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(InitWalletResp {
                ok: true,
                name: req.name,
            }),
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn wallet_address(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if !is_authorized(&headers, &state.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let url = format!("{}/wallet/{}", state.rpc_url.trim_end_matches('/'), name);
    let client = match Client::new(&url, state.auth.clone()) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match Wallet::new_address(&client) {
        Ok(addr) => (StatusCode::OK, Json(AddressResp { address: addr })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn wallet_balance(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if !is_authorized(&headers, &state.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let url = format!("{}/wallet/{}", state.rpc_url.trim_end_matches('/'), name);
    let client = match Client::new(&url, state.auth.clone()) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match Wallet::balance(&client) {
        Ok(btc) => (StatusCode::OK, Json(BalanceResp { balance_btc: btc })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn create_collection(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<CreateCollectionReq>,
) -> impl IntoResponse {
    if !is_authorized(&headers, &state.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let mut laos = [0u8; 20];
    let Ok(bytes) = hex::decode(&req.laos_hex) else {
        return (StatusCode::BAD_REQUEST, "invalid laos_hex").into_response();
    };
    if bytes.len() != 20 {
        return (StatusCode::BAD_REQUEST, "laos_hex must be 20 bytes hex").into_response();
    }
    laos.copy_from_slice(&bytes);

    let url = format!(
        "{}/wallet/{}",
        state.rpc_url.trim_end_matches('/'),
        req.wallet
    );
    let client = match Client::new(&url, state.auth.clone()) {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    match Wallet::create_and_broadcast_collection(
        &client,
        laos,
        req.rebaseable,
        req.fee_rate,
    ) {
        Ok(txid) => (
            StatusCode::OK,
            Json(CreateCollectionResp { txid }),
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

pub async fn serve(
    bind: String,
    rpc_url: String,
    auth: Auth,
    token: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = ApiState { rpc_url, auth, token };

    let app = Router::new()
        .route("/wallet/init", post(wallet_init))
        .route("/wallet/:name/address", get(wallet_address))
        .route("/wallet/:name/balance", get(wallet_balance))
        .route("/collections", post(create_collection))
        .with_state(state);

    let addr: SocketAddr = bind.parse()?;
    println!("HTTP API listening on http://{}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}
