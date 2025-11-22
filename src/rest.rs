use std::net::SocketAddr;

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::storage::Storage;

#[derive(Clone)]
pub struct AppState<S: Storage> {
    pub storage: S,
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
    id: String,
    evm_collection_address: String,
    rebaseable: bool,
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

pub async fn serve<S: Storage + Clone + Send + Sync + 'static>(
    addr: SocketAddr,
    storage: S,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    let state = AppState {
        storage,
        started_at: std::time::SystemTime::now(),
    };

    let app = Router::new()
        .route("/health", get(health::<S>))
        .route("/state", get(chain_state::<S>))
        .route("/collections", get(list_collections::<S>))
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

async fn health<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
) -> impl IntoResponse {
    let uptime_secs = state.started_at.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok",
            uptime_secs,
        }),
    )
}

async fn chain_state<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
) -> impl IntoResponse {
    let last = state.storage.load_last().ok().flatten().map(|b| LastBlock {
        height: b.height,
        hash: b.hash,
    });
    Json(ChainStateResponse { last })
}

async fn list_collections<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
) -> impl IntoResponse {
    let collections = state
        .storage
        .list_collections()
        .unwrap_or_default()
        .into_iter()
        .map(
            |(key, evm_collection_address, rebaseable)| CollectionResponse {
                id: key.id,
                evm_collection_address,
                rebaseable,
            },
        )
        .collect();
    Json(CollectionsResponse { collections })
}
