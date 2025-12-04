use std::{fmt, net::SocketAddr, str::FromStr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::{
    storage::{
        traits::{Collection, CollectionKey},
        Storage,
    },
    types::{Brc721Error, Brc721Token},
};

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
#[serde(rename_all = "camelCase")]
struct TokenOwnerResponse {
    collection_id: String,
    token_id: String,
    ownership_status: OwnershipStatus,
    owner: TokenOwnerDetails,
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum OwnershipStatus {
    InitialOwner,
    #[allow(dead_code)]
    RegisteredOwner,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
enum TokenOwnerDetails {
    InitialOwner {
        #[serde(rename = "h160Address")]
        h160_address: String,
    },
    #[allow(dead_code)]
    RegisteredOwner {
        #[serde(rename = "outpoint")]
        outpoint: String,
    },
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
            "/api/v1/brc721/collections/:collection_id/tokens/:token_id/owner",
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
        .map(collection_to_response)
        .collect();
    Json(CollectionsResponse { collections })
}

async fn get_collection<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let key = match CollectionKey::from_str(&id) {
        Ok(key) => key,
        Err(err) => {
            log::warn!("Invalid collection id {}: {}", id, err);
            return StatusCode::BAD_REQUEST.into_response();
        }
    };
    match state.storage.load_collection(&key) {
        Ok(Some(collection)) => Json(collection_to_response(collection)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            log::error!("Failed to load collection {}: {:?}", id, err);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn collection_to_response(collection: Collection) -> CollectionResponse {
    CollectionResponse {
        id: collection.key.to_string(),
        evm_collection_address: format!("{:#x}", collection.evm_collection_address),
        rebaseable: collection.rebaseable,
    }
}

async fn get_token_owner<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
    Path((collection_id, token_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let key = match CollectionKey::from_str(&collection_id) {
        Ok(key) => key,
        Err(err) => {
            log::warn!("Invalid collection id {}: {}", collection_id, err);
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    match state.storage.load_collection(&key) {
        Ok(Some(_)) => {}
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            log::error!("Failed to load collection {}: {:?}", key, err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }

    let token = match parse_token_id(&token_id) {
        Ok(token) => token,
        Err(err) => {
            log::warn!("Invalid token id {}: {}", token_id, err);
            return StatusCode::BAD_REQUEST.into_response();
        }
    };

    Json(TokenOwnerResponse {
        collection_id: key.to_string(),
        token_id: format!("0x{}", hex::encode(token.to_bytes())),
        ownership_status: OwnershipStatus::InitialOwner,
        owner: TokenOwnerDetails::InitialOwner {
            h160_address: format!("{:#x}", token.h160_address()),
        },
    })
    .into_response()
}

fn parse_token_id(token_id: &str) -> Result<Brc721Token, TokenIdParseError> {
    let trimmed = token_id.trim();
    if trimmed.is_empty() {
        return Err(TokenIdParseError::Empty);
    }

    let hex_part = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    if hex_part.is_empty() {
        return Err(TokenIdParseError::Empty);
    }

    let normalized = if hex_part.len() % 2 != 0 {
        let mut padded = String::with_capacity(hex_part.len() + 1);
        padded.push('0');
        padded.push_str(hex_part);
        padded
    } else {
        hex_part.to_owned()
    };

    let byte_len = normalized.len() / 2;
    if byte_len > Brc721Token::LEN {
        return Err(TokenIdParseError::TooLong(byte_len));
    }

    let decoded = hex::decode(&normalized).map_err(TokenIdParseError::InvalidHex)?;
    let mut padded = [0u8; Brc721Token::LEN];
    let start = Brc721Token::LEN - decoded.len();
    padded[start..].copy_from_slice(&decoded);

    Brc721Token::try_from(padded).map_err(TokenIdParseError::TokenDecode)
}

#[derive(Debug)]
enum TokenIdParseError {
    Empty,
    TooLong(usize),
    InvalidHex(hex::FromHexError),
    TokenDecode(Brc721Error),
}

impl fmt::Display for TokenIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenIdParseError::Empty => write!(f, "token id is empty"),
            TokenIdParseError::TooLong(len) => {
                write!(f, "token id is too long: {} bytes", len)
            }
            TokenIdParseError::InvalidHex(err) => write!(f, "invalid hex token id: {}", err),
            TokenIdParseError::TokenDecode(err) => write!(f, "invalid token encoding: {}", err),
        }
    }
}

impl std::error::Error for TokenIdParseError {}

#[cfg(test)]
mod tests {
    use super::*;
    use ethereum_types::H160;

    fn sample_address() -> H160 {
        H160::from([
            0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ])
    }

    #[test]
    fn parse_token_id_accepts_prefixed_hex() {
        let slot: u128 = 0x0000_abcd_ef01_2345_6789_abcd;
        let token = Brc721Token::new(slot, sample_address()).expect("valid token");
        let encoded = format!("0x{}", hex::encode(token.to_bytes()));

        let parsed = parse_token_id(&encoded).expect("parse success");
        assert_eq!(parsed, token);
    }

    #[test]
    fn parse_token_id_accepts_unprefixed_and_short_hex() {
        let slot: u128 = 42;
        let token = Brc721Token::new(slot, sample_address()).expect("valid token");
        let mut encoded = hex::encode(token.to_bytes());
        while encoded.starts_with('0') {
            encoded.remove(0);
        }

        let parsed = parse_token_id(&encoded).expect("parse success");
        assert_eq!(parsed, token);
    }

    #[test]
    fn parse_token_id_rejects_oversized_values() {
        let oversized = "11".repeat(Brc721Token::LEN + 1);
        match parse_token_id(&oversized) {
            Err(TokenIdParseError::TooLong(bytes)) => {
                assert_eq!(bytes, Brc721Token::LEN + 1);
            }
            other => panic!("expected TooLong error, got {:?}", other),
        }
    }
}
