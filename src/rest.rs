use std::net::SocketAddr;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::storage::Storage;

#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn Storage + Send + Sync>,
    pub started_at: std::time::SystemTime,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    uptime_secs: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChainStateResponse {
    last: Option<LastBlock>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectionResponse {
    block_height: u64,
    tx_index: u32,
    owner: String,
    params: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectionsResponse {
    collections: Vec<CollectionResponse>,
}

#[derive(Serialize)]
struct LastBlock {
    height: u64,
    hash: String,
}

pub async fn serve(
    addr: SocketAddr,
    storage: Arc<dyn Storage + Send + Sync>,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    let state = AppState {
        storage,
        started_at: std::time::SystemTime::now(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/state", get(chain_state))
        .route("/collections", get(list_collections))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    log::info!("🌐 REST listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
            log::info!("🛑 REST shutdown requested");
        })
        .await?;
    log::info!("👋 REST server exited");
    Ok(())
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let uptime_secs = state.started_at.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok",
            uptime_secs,
        }),
    )
}

async fn chain_state(State(state): State<AppState>) -> impl IntoResponse {
    let last = state.storage.load_last().ok().flatten().map(|b| LastBlock {
        height: b.height,
        hash: b.hash,
    });
    Json(ChainStateResponse { last })
}

async fn list_collections(State(state): State<AppState>) -> impl IntoResponse {
    let collections = state
        .storage
        .list_collections()
        .unwrap_or_default()
        .into_iter()
        .map(|(key, owner, params)| CollectionResponse {
            block_height: key.block_height,
            tx_index: key.tx_index,
            owner,
            params,
        })
        .collect();
    Json(CollectionsResponse { collections })
}
