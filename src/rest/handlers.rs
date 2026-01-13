use std::{fmt, str::FromStr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use ethereum_types::{H160, U256};

use crate::{
    storage::{
        traits::{Collection, CollectionKey},
        Storage,
    },
    types::{Brc721Error, Brc721Token},
};

use super::{
    models::{
        AddressAssetsResponse, ChainStateResponse, CollectionResponse, CollectionsResponse,
        ErrorResponse, HealthResponse, LastBlock, OwnershipStatus, OwnershipUtxoResponse,
        SlotRangeResponse, TokenOwnerResponse,
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

    match state.storage.find_unspent_ownership_utxo_for_slot(
        &key,
        token.h160_address(),
        token.slot_number(),
    ) {
        Ok(Some(utxo)) => Json(TokenOwnerResponse {
            collection_id: key.to_string(),
            token_id,
            ownership_status: OwnershipStatus::RegisteredOwner,
            owner_h160: format!("{:#x}", utxo.owner_h160),
            txid: Some(utxo.reg_txid),
            vout: Some(utxo.reg_vout),
        })
        .into_response(),
        Ok(None) => Json(TokenOwnerResponse {
            collection_id: key.to_string(),
            token_id,
            ownership_status: OwnershipStatus::InitialOwner,
            owner_h160: initial_owner_h160,
            txid: None,
            vout: None,
        })
        .into_response(),
        Err(err) => {
            log::error!(
                "Failed to resolve token owner for collection {} token {}: {:?}",
                key,
                token_id,
                err
            );
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn get_address_assets<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    use bitcoin::hashes::{hash160, Hash};

    let address = match bitcoin::Address::from_str(&address) {
        Ok(address) => address.assume_checked(),
        Err(err) => {
            log::warn!("Invalid address {}: {}", address, err);
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    message: "invalid address".to_string(),
                }),
            )
                .into_response();
        }
    };

    let script_pubkey = address.script_pubkey();
    let hash = hash160::Hash::hash(script_pubkey.as_bytes());
    let owner_h160 = H160::from_slice(hash.as_byte_array());

    let utxos = match state
        .storage
        .list_unspent_ownership_utxos_by_owner(owner_h160)
    {
        Ok(utxos) => utxos,
        Err(err) => {
            log::error!(
                "Failed to list ownership UTXOs for address {}: {:?}",
                address,
                err
            );
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut owned = Vec::with_capacity(utxos.len());
    for utxo in utxos {
        let ranges = match state
            .storage
            .list_ownership_ranges(&utxo.reg_txid, utxo.reg_vout)
        {
            Ok(ranges) => ranges,
            Err(err) => {
                log::error!(
                    "Failed to list ownership ranges for outpoint {}:{}: {:?}",
                    utxo.reg_txid,
                    utxo.reg_vout,
                    err
                );
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };

        owned.push(OwnershipUtxoResponse {
            collection_id: utxo.collection_id.to_string(),
            txid: utxo.reg_txid,
            vout: utxo.reg_vout,
            init_owner_h160: format!("{:#x}", utxo.base_h160),
            created_height: utxo.created_height,
            created_tx_index: utxo.created_tx_index,
            slot_ranges: ranges
                .into_iter()
                .map(|range| SlotRangeResponse {
                    start: range.slot_start.to_string(),
                    end: range.slot_end.to_string(),
                })
                .collect(),
        });
    }

    owned.sort_by(|a, b| {
        a.collection_id
            .cmp(&b.collection_id)
            .then_with(|| a.txid.cmp(&b.txid))
            .then_with(|| a.vout.cmp(&b.vout))
    });

    Json(AddressAssetsResponse {
        address: address.to_string(),
        address_h160: format!("{:#x}", owner_h160),
        utxos: owned,
    })
    .into_response()
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
            Block, Collection, CollectionKey, OwnershipRange, OwnershipUtxo, OwnershipUtxoSave,
            StorageRead, StorageTx, StorageWrite,
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
        assert_eq!(payload.txid, None);
        assert_eq!(payload.vout, None);
    }

    #[tokio::test]
    async fn get_token_owner_returns_registered_owner_payload_when_registered() {
        let collection = sample_collection();
        let token = sample_token();
        let token_decimal = format_token_id(&token);
        let collection_id = collection.key.to_string();

        let registered_owner = H160::from_low_u64_be(0x1234);
        let utxo = OwnershipUtxo {
            collection_id: collection.key.clone(),
            reg_txid: "txid".to_string(),
            reg_vout: 1,
            owner_h160: registered_owner,
            base_h160: token.h160_address(),
            created_height: 840_001,
            created_tx_index: 2,
            spent_txid: None,
            spent_height: None,
            spent_tx_index: None,
        };

        let storage = TestStorage::with_collection(collection).with_ownership_utxo(
            utxo,
            vec![OwnershipRange {
                slot_start: token.slot_number(),
                slot_end: token.slot_number(),
            }],
        );

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
        assert_eq!(payload.owner_h160, format!("{:#x}", registered_owner));
        assert_eq!(payload.txid.as_deref(), Some("txid"));
        assert_eq!(payload.vout, Some(1));
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
        ownership_utxos: Arc<RwLock<Vec<OwnershipUtxo>>>,
        ownership_ranges: Arc<RwLock<Vec<(String, u32, OwnershipRange)>>>,
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

        fn with_ownership_utxo(self, utxo: OwnershipUtxo, ranges: Vec<OwnershipRange>) -> Self {
            {
                let mut guard = self.ownership_utxos.write().unwrap();
                guard.push(utxo.clone());
            }
            {
                let mut guard = self.ownership_ranges.write().unwrap();
                for range in ranges {
                    guard.push((utxo.reg_txid.clone(), utxo.reg_vout, range));
                }
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

        fn list_ownership_ranges(
            &self,
            reg_txid: &str,
            reg_vout: u32,
        ) -> anyhow::Result<Vec<OwnershipRange>> {
            let ranges = self.ownership_ranges.read().unwrap();
            Ok(ranges
                .iter()
                .filter(|(txid, vout, _)| txid == reg_txid && *vout == reg_vout)
                .map(|(_, _, range)| range.clone())
                .collect())
        }

        fn find_unspent_ownership_utxo_for_slot(
            &self,
            collection_id: &CollectionKey,
            base_h160: H160,
            slot: u128,
        ) -> anyhow::Result<Option<OwnershipUtxo>> {
            let utxos = self.ownership_utxos.read().unwrap();
            let ranges = self.ownership_ranges.read().unwrap();
            for utxo in utxos.iter() {
                if &utxo.collection_id != collection_id {
                    continue;
                }
                if utxo.base_h160 != base_h160 {
                    continue;
                }
                if utxo.spent_txid.is_some() {
                    continue;
                }
                let covers = ranges.iter().any(|(txid, vout, range)| {
                    txid == &utxo.reg_txid
                        && *vout == utxo.reg_vout
                        && range.slot_start <= slot
                        && range.slot_end >= slot
                });
                if covers {
                    return Ok(Some(utxo.clone()));
                }
            }
            Ok(None)
        }

        fn list_unspent_ownership_utxos_by_owner(
            &self,
            owner_h160: H160,
        ) -> anyhow::Result<Vec<OwnershipUtxo>> {
            let utxos = self.ownership_utxos.read().unwrap();
            Ok(utxos
                .iter()
                .filter(|utxo| utxo.spent_txid.is_none() && utxo.owner_h160 == owner_h160)
                .cloned()
                .collect())
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

        fn list_ownership_ranges(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> anyhow::Result<Vec<OwnershipRange>> {
            Err(anyhow!("not implemented"))
        }

        fn find_unspent_ownership_utxo_for_slot(
            &self,
            _collection_id: &CollectionKey,
            _base_h160: H160,
            _slot: u128,
        ) -> anyhow::Result<Option<OwnershipUtxo>> {
            Err(anyhow!("not implemented"))
        }

        fn list_unspent_ownership_utxos_by_owner(
            &self,
            _owner_h160: H160,
        ) -> anyhow::Result<Vec<OwnershipUtxo>> {
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

        fn save_ownership_utxo(&self, _utxo: OwnershipUtxoSave<'_>) -> anyhow::Result<()> {
            Err(anyhow!("not implemented"))
        }

        fn save_ownership_range(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _slot_start: u128,
            _slot_end: u128,
        ) -> anyhow::Result<()> {
            Err(anyhow!("not implemented"))
        }

        fn mark_ownership_utxo_spent(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
            _spent_txid: &str,
            _spent_height: u64,
            _spent_tx_index: u32,
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
