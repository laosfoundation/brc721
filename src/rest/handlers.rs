use std::{fmt, str::FromStr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::{
    storage::{
        traits::{Collection, CollectionKey},
        Storage,
    },
    types::{Brc721Error, Brc721Token},
};

use super::{
    models::{
        ChainStateResponse, CollectionResponse, CollectionsResponse, HealthResponse, LastBlock,
        OwnershipStatus, TokenOwnerResponse,
    },
    AppState,
};

pub async fn health<S: Storage + Clone + Send + Sync + 'static>(
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

pub async fn chain_state<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
) -> impl IntoResponse {
    let last = state.storage.load_last().ok().flatten().map(|b| LastBlock {
        height: b.height,
        hash: b.hash,
    });
    Json(ChainStateResponse { last })
}

pub async fn list_collections<S: Storage + Clone + Send + Sync + 'static>(
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

pub async fn get_collection<S: Storage + Clone + Send + Sync + 'static>(
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

pub async fn get_token_owner<S: Storage + Clone + Send + Sync + 'static>(
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
        h160_address: format!("{:#x}", token.h160_address()),
    })
    .into_response()
}

fn collection_to_response(collection: Collection) -> CollectionResponse {
    CollectionResponse {
        id: collection.key.to_string(),
        evm_collection_address: format!("{:#x}", collection.evm_collection_address),
        rebaseable: collection.rebaseable,
    }
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

    let normalized = if hex_part.len().rem_euclid(2) != 0 {
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
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use ethereum_types::H160;
    use http_body_util::BodyExt;
    use std::{
        sync::{Arc, RwLock},
        time::SystemTime,
    };
    use tower::ServiceExt;

    use crate::storage::{
        traits::{Block, Collection, CollectionKey, StorageRead, StorageTx, StorageWrite},
        Storage,
    };
    use anyhow::anyhow;

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

    #[tokio::test]
    async fn get_token_owner_returns_initial_owner_payload() {
        let collection = sample_collection();
        let storage = TestStorage::with_collection(collection.clone());
        let token = sample_token();
        let token_hex = format!("0x{}", hex::encode(token.to_bytes()));
        let collection_id = collection.key.to_string();

        let response = issue_owner_request(storage, &collection_id, &token_hex).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let payload: TokenOwnerResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(payload.collection_id, collection_id);
        assert_eq!(payload.token_id, token_hex);
        assert_eq!(payload.h160_address, format!("{:#x}", token.h160_address()));
    }

    #[tokio::test]
    async fn get_token_owner_rejects_bad_token_id() {
        let collection = sample_collection();
        let storage = TestStorage::with_collection(collection.clone());
        let response = issue_owner_request(storage, &collection.key.to_string(), "not-hex").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_token_owner_returns_404_for_unknown_collection() {
        let storage = TestStorage::default();
        let token_hex = format!("0x{}", hex::encode(sample_token().to_bytes()));
        let response = issue_owner_request(storage, "850123:0", &token_hex).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    fn sample_token() -> Brc721Token {
        Brc721Token::new(42, sample_address()).expect("valid token")
    }

    fn sample_collection() -> Collection {
        Collection {
            key: CollectionKey::new(850123, 0),
            evm_collection_address: sample_address(),
            rebaseable: false,
        }
    }

    async fn issue_owner_request(
        storage: TestStorage,
        collection_id: &str,
        token_id: &str,
    ) -> axum::response::Response {
        let router = Router::new()
            .route(
                "/collections/:collection_id/tokens/:token_id/owner",
                get(get_token_owner::<TestStorage>),
            )
            .with_state(AppState {
                storage,
                started_at: SystemTime::now(),
            });

        router
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/collections/{}/tokens/{}/owner",
                        collection_id, token_id
                    ))
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[derive(Clone, Default)]
    struct TestStorage {
        collections: Arc<RwLock<Vec<Collection>>>,
    }

    impl TestStorage {
        fn with_collection(collection: Collection) -> Self {
            let storage = Self::default();
            {
                let mut guard = storage.collections.write().unwrap();
                guard.push(collection);
            }
            storage
        }
    }

    impl StorageRead for TestStorage {
        fn load_last(&self) -> anyhow::Result<Option<Block>> {
            Ok(None)
        }

        fn load_collection(&self, id: &CollectionKey) -> anyhow::Result<Option<Collection>> {
            let collections = self.collections.read().unwrap();
            Ok(collections.iter().find(|c| &c.key == id).cloned())
        }

        fn list_collections(&self) -> anyhow::Result<Vec<Collection>> {
            Ok(self.collections.read().unwrap().clone())
        }
    }

    impl Storage for TestStorage {
        type Tx = NoopTx;

        fn begin_tx(&self) -> anyhow::Result<Self::Tx> {
            Err(anyhow!("transactions not supported in test storage"))
        }
    }

    struct NoopTx;

    impl StorageRead for NoopTx {
        fn load_last(&self) -> anyhow::Result<Option<Block>> {
            Err(anyhow!("not implemented"))
        }

        fn load_collection(&self, _id: &CollectionKey) -> anyhow::Result<Option<Collection>> {
            Err(anyhow!("not implemented"))
        }

        fn list_collections(&self) -> anyhow::Result<Vec<Collection>> {
            Err(anyhow!("not implemented"))
        }
    }

    impl StorageWrite for NoopTx {
        fn save_last(&self, _height: u64, _hash: &str) -> anyhow::Result<()> {
            Err(anyhow!("not implemented"))
        }

        fn save_collection(
            &self,
            _key: CollectionKey,
            _evm_collection_address: H160,
            _rebaseable: bool,
        ) -> anyhow::Result<()> {
            Err(anyhow!("not implemented"))
        }
    }

    impl StorageTx for NoopTx {
        fn commit(self) -> anyhow::Result<()> {
            Ok(())
        }
    }
}
