use std::{fmt, str::FromStr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use ethereum_types::U256;

use crate::{
    storage::{
        traits::{Collection, CollectionKey},
        Storage,
    },
    types::{Brc721Error, Brc721Token},
};

use super::{
    models::{
        ChainStateResponse, CollectionResponse, CollectionsResponse, ErrorResponse, HealthResponse,
        LastBlock, OwnershipStatus, TokenOwnerResponse,
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
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    message: "invalid token id".to_string(),
                }),
            )
                .into_response();
        }
    };

    let token_id = format_token_id(&token);
    let initial_owner_h160 = format_owner_h160(&token);

    match state.storage.load_registered_token(&key, &token_id) {
        Ok(Some(registered)) => {
            if registered.owner_h160 != token.h160_address() {
                log::warn!(
                    "Registered token owner mismatch: collection={} token_id={} token_owner={:#x} stored_owner={:#x}",
                    key,
                    token_id,
                    token.h160_address(),
                    registered.owner_h160,
                );
            }

            Json(TokenOwnerResponse {
                collection_id: key.to_string(),
                token_id,
                ownership_status: OwnershipStatus::RegisteredOwner,
                owner_h160: format!("{:#x}", registered.owner_h160),
            })
            .into_response()
        }
        Ok(None) => Json(TokenOwnerResponse {
            collection_id: key.to_string(),
            token_id,
            ownership_status: OwnershipStatus::InitialOwner,
            owner_h160: initial_owner_h160,
        })
        .into_response(),
        Err(err) => {
            log::error!(
                "Failed to load registered token for collection {} token {}: {:?}",
                key,
                token_id,
                err
            );
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            message: "endpoint not found".to_string(),
        }),
    )
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

    let value = U256::from_dec_str(trimmed).map_err(TokenIdParseError::InvalidDecimal)?;

    Brc721Token::try_from(value).map_err(TokenIdParseError::TokenDecode)
}

fn format_token_id(token: &Brc721Token) -> String {
    token.to_u256().to_string()
}

fn format_owner_h160(token: &Brc721Token) -> String {
    format!("{:#x}", token.h160_address())
}

#[derive(Debug)]
enum TokenIdParseError {
    Empty,
    InvalidDecimal(U256FromDecStrError),
    TokenDecode(Brc721Error),
}

type U256FromDecStrError = ethereum_types::FromDecStrErr;

impl fmt::Display for TokenIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenIdParseError::Empty => write!(f, "token id is empty"),
            TokenIdParseError::InvalidDecimal(err) => {
                write!(f, "invalid decimal token id: {}", err)
            }
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
    use ethereum_types::{H160, U256};
    use http_body_util::BodyExt;
    use std::{
        sync::{Arc, RwLock},
        time::SystemTime,
    };
    use tower::ServiceExt;

    use crate::storage::{
        traits::{
            Block, Collection, CollectionKey, RegisteredToken, RegisteredTokenSave, StorageRead,
            StorageTx, StorageWrite,
        },
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
    fn parse_token_id_accepts_decimal() {
        let slot: u128 = 0x0000_abcd_ef01_2345_6789_abcd;
        let token = Brc721Token::new(slot, sample_address()).expect("valid token");
        let encoded = token.to_u256().to_string();

        let parsed = parse_token_id(&encoded).expect("parse success");
        assert_eq!(parsed, token);
    }

    #[test]
    fn parse_token_id_rejects_oversized_values() {
        let oversized = format!("{}0", U256::MAX);
        let err = parse_token_id(&oversized).unwrap_err();
        assert!(matches!(err, TokenIdParseError::InvalidDecimal(_)));
    }

    #[tokio::test]
    async fn get_token_owner_returns_initial_owner_payload() {
        let collection = sample_collection();
        let storage = TestStorage::with_collection(collection.clone());
        let token = sample_token();
        let token_decimal = format_token_id(&token);
        let expected_owner_h160 = format_owner_h160(&token);
        let collection_id = collection.key.to_string();

        let response = issue_owner_request(storage, &collection_id, &token_decimal).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let payload: TokenOwnerResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(payload.collection_id, collection_id);
        assert_eq!(payload.token_id, token_decimal);
        assert!(matches!(
            payload.ownership_status,
            OwnershipStatus::InitialOwner
        ));
        assert_eq!(payload.owner_h160, expected_owner_h160);
    }

    #[tokio::test]
    async fn get_token_owner_returns_registered_owner_payload_when_registered() {
        let collection = sample_collection();
        let token = sample_token();
        let token_decimal = format_token_id(&token);
        let collection_id = collection.key.to_string();

        let registered = RegisteredToken {
            collection_id: collection.key.clone(),
            token_id: token_decimal.clone(),
            owner_h160: sample_address(),
            reg_txid: "txid".to_string(),
            reg_vout: 1,
            created_height: 840_001,
            created_tx_index: 2,
        };

        let storage = TestStorage::with_collection(collection).with_registered_token(registered);

        let response = issue_owner_request(storage, &collection_id, &token_decimal).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let payload: TokenOwnerResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(payload.collection_id, collection_id);
        assert_eq!(payload.token_id, token_decimal);
        assert!(matches!(
            payload.ownership_status,
            OwnershipStatus::RegisteredOwner
        ));
        assert_eq!(payload.owner_h160, format!("{:#x}", sample_address()));
    }

    #[tokio::test]
    async fn get_token_owner_rejects_bad_token_id() {
        let collection = sample_collection();
        let storage = TestStorage::with_collection(collection.clone());
        let response =
            issue_owner_request(storage, &collection.key.to_string(), "not-a-number").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let payload: ErrorResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(payload.message, "invalid token id");
    }

    #[tokio::test]
    async fn get_token_owner_returns_404_for_unknown_collection() {
        let storage = TestStorage::default();
        let token_decimal = format_token_id(&sample_token());
        let response = issue_owner_request(storage, "850123:0", &token_decimal).await;
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
                "/collections/:collection_id/tokens/:token_id",
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
                        "/collections/{}/tokens/{}",
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
        registered_tokens: Arc<RwLock<Vec<RegisteredToken>>>,
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

        fn with_registered_token(self, token: RegisteredToken) -> Self {
            {
                let mut guard = self.registered_tokens.write().unwrap();
                guard.push(token);
            }
            self
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

        fn load_registered_token(
            &self,
            collection_id: &CollectionKey,
            token_id: &str,
        ) -> anyhow::Result<Option<RegisteredToken>> {
            let registered = self.registered_tokens.read().unwrap();
            Ok(registered
                .iter()
                .find(|entry| &entry.collection_id == collection_id && entry.token_id == token_id)
                .cloned())
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

        fn load_registered_token(
            &self,
            _collection_id: &CollectionKey,
            _token_id: &str,
        ) -> anyhow::Result<Option<RegisteredToken>> {
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

        fn save_registered_token(&self, _token: RegisteredTokenSave<'_>) -> anyhow::Result<()> {
            Err(anyhow!("not implemented"))
        }
    }

    impl StorageTx for NoopTx {
        fn commit(self) -> anyhow::Result<()> {
            Ok(())
        }
    }
}
