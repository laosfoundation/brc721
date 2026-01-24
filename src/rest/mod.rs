use std::net::SocketAddr;

use axum::{routing::get, Router};
use bitcoin::Network;

use crate::storage::Storage;

mod handlers;
mod models;

use handlers::{
    chain_state, get_address_assets, get_collection, get_token_owner, get_utxo_assets, health,
    list_collections, not_found,
};

#[derive(Clone)]
pub struct AppState<S: Storage> {
    pub storage: S,
    pub started_at: std::time::SystemTime,
    pub network: Network,
}

pub async fn serve<S: Storage + Clone + Send + Sync + 'static>(
    addr: SocketAddr,
    storage: S,
    network: Network,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    log::info!("🌐 REST service on http://{}", addr);

    let state = AppState {
        storage,
        started_at: std::time::SystemTime::now(),
        network,
    };

    let app = Router::new()
        .route("/health", get(health::<S>))
        .route("/state", get(chain_state::<S>))
        .route("/collections/:id", get(get_collection::<S>))
        .route("/collections", get(list_collections::<S>))
        .route("/addresses/:address/assets", get(get_address_assets::<S>))
        .route("/utxos/:txid/:vout/assets", get(get_utxo_assets::<S>))
        .route(
            "/collections/:collection_id/tokens/:token_id",
            get(get_token_owner::<S>),
        )
        .fallback(not_found)
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
