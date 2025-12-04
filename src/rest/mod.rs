use std::net::SocketAddr;

use axum::{routing::get, Router};

use crate::storage::Storage;

mod handlers;
mod models;

use handlers::{chain_state, get_collection, get_token_owner, health, list_collections};

#[derive(Clone)]
pub struct AppState<S: Storage> {
    pub storage: S,
    pub started_at: std::time::SystemTime,
}

pub async fn serve<S: Storage + Clone + Send + Sync + 'static>(
    addr: SocketAddr,
    storage: S,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    log::info!("🌐 REST service on http://{}", addr);

    let state = AppState {
        storage,
        started_at: std::time::SystemTime::now(),
    };

    let app = Router::new()
        .route("/health", get(health::<S>))
        .route("/state", get(chain_state::<S>))
        .route("/collection/:id", get(get_collection::<S>))
        .route("/collections", get(list_collections::<S>))
        .route(
            "/collections/:collection_id/tokens/:token_id/owner",
            get(get_token_owner::<S>),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
            log::info!("🛑 REST shutdown requested");
        })
        .await?;
    log::info!("👋 REST server exited");
    Ok(())
}
