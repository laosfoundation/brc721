use std::{fmt, str::FromStr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use ethereum_types::U256;

use crate::{
    storage::{
        traits::{Collection, CollectionKey},
        Storage,
    },
    types::{h160_from_script_pubkey, Brc721Error, Brc721Token},
};

use super::{
    models::{
        AddressAssetsResponse, ChainStateResponse, CollectionResponse, CollectionsResponse,
        ErrorResponse, HealthResponse, LastBlock, OwnershipStatus, OwnershipUtxoResponse,
        SlotRangeResponse, TokenOwnerResponse, UtxoAssetsResponse, UtxoOwnershipResponse,
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
    match state.storage.load_last() {
        Ok(last) => {
            let last = last.map(|b| LastBlock {
                height: b.height,
                hash: b.hash,
            });
            Json(ChainStateResponse { last }).into_response()
        }
        Err(err) => {
            log::error!("Failed to load chain state: {:?}", err);
            internal_error()
        }
    }
}

pub async fn list_collections<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
) -> impl IntoResponse {
    let collections = match state.storage.list_collections() {
        Ok(collections) => collections
            .into_iter()
            .map(collection_to_response)
            .collect(),
        Err(err) => {
            log::error!("Failed to list collections: {:?}", err);
            return internal_error();
        }
    };
    Json(CollectionsResponse { collections }).into_response()
}

pub async fn get_collection<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let key = match CollectionKey::from_str(&id) {
        Ok(key) => key,
        Err(err) => {
            log::warn!("Invalid collection id {}: {}", id, err);
            return json_error(StatusCode::BAD_REQUEST, "invalid collection id");
        }
    };
    match state.storage.load_collection(&key) {
        Ok(Some(collection)) => Json(collection_to_response(collection)).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "collection not found"),
        Err(err) => {
            log::error!("Failed to load collection {}: {:?}", id, err);
            internal_error()
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
            return json_error(StatusCode::BAD_REQUEST, "invalid collection id");
        }
    };

    match state.storage.load_collection(&key) {
        Ok(Some(_)) => {}
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "collection not found"),
        Err(err) => {
            log::error!("Failed to load collection {}: {:?}", key, err);
            return internal_error();
        }
    }

    let token = match parse_token_id(&token_id) {
        Ok(token) => token,
        Err(err) => {
            log::warn!("Invalid token id {}: {}", token_id, err);
            return json_error(StatusCode::BAD_REQUEST, "invalid token id");
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
            height: key.block_height,
            tx_index: key.tx_index,
            token_id,
            ownership_status: OwnershipStatus::RegisteredOwner,
            owner_h160: format!("{:#x}", utxo.owner_h160),
            txid: Some(utxo.reg_txid),
            vout: Some(utxo.reg_vout),
            utxo_height: Some(utxo.created_height),
            utxo_tx_index: Some(utxo.created_tx_index),
        })
        .into_response(),
        Ok(None) => Json(TokenOwnerResponse {
            collection_id: key.to_string(),
            height: key.block_height,
            tx_index: key.tx_index,
            token_id,
            ownership_status: OwnershipStatus::InitialOwner,
            owner_h160: initial_owner_h160,
            txid: None,
            vout: None,
            utxo_height: None,
            utxo_tx_index: None,
        })
        .into_response(),
        Err(err) => {
            log::error!(
                "Failed to resolve token owner for collection {} token {}: {:?}",
                key,
                token_id,
                err
            );
            internal_error()
        }
    }
}

pub async fn get_address_assets<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    let address = match bitcoin::Address::from_str(&address) {
        Ok(address) => address.assume_checked(),
        Err(err) => {
            log::warn!("Invalid address {}: {}", address, err);
            return json_error(StatusCode::BAD_REQUEST, "invalid address");
        }
    };

    let script_pubkey = address.script_pubkey();
    let owner_h160 = h160_from_script_pubkey(&script_pubkey);

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
            return internal_error();
        }
    };

    let mut owned = Vec::with_capacity(utxos.len());
    for utxo in utxos {
        let ranges = match state.storage.list_ownership_ranges(&utxo) {
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
            utxo_height: utxo.created_height,
            utxo_tx_index: utxo.created_tx_index,
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
        owner_h160: format!("{:#x}", owner_h160),
        utxos: owned,
    })
    .into_response()
}

pub async fn get_utxo_assets<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
    Path((txid, vout_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let _ = match bitcoin::Txid::from_str(&txid) {
        Ok(txid) => txid,
        Err(err) => {
            log::warn!("Invalid txid {}: {}", txid, err);
            return json_error(StatusCode::BAD_REQUEST, "invalid txid");
        }
    };

    let vout: u32 = match vout_str.parse() {
        Ok(vout) => vout,
        Err(_) => {
            return json_error(StatusCode::BAD_REQUEST, "invalid vout");
        }
    };

    let utxos = match state
        .storage
        .list_unspent_ownership_utxos_by_outpoint(&txid, vout)
    {
        Ok(utxos) => utxos,
        Err(err) => {
            log::error!(
                "Failed to list ownership UTXOs for outpoint {}:{}: {:?}",
                txid,
                vout,
                err
            );
            return internal_error();
        }
    };

    if utxos.is_empty() {
        return json_error(StatusCode::NOT_FOUND, "utxo not found");
    }

    let owner_h160 = format!("{:#x}", utxos[0].owner_h160);
    let utxo_height = utxos[0].created_height;
    let utxo_tx_index = utxos[0].created_tx_index;
    let mut assets = Vec::with_capacity(utxos.len());
    for utxo in utxos {
        let ranges = match state.storage.list_ownership_ranges(&utxo) {
            Ok(ranges) => ranges,
            Err(err) => {
                log::error!(
                    "Failed to list ownership ranges for outpoint {}:{}: {:?}",
                    utxo.reg_txid,
                    utxo.reg_vout,
                    err
                );
                return internal_error();
            }
        };

        assets.push(UtxoOwnershipResponse {
            collection_id: utxo.collection_id.to_string(),
            init_owner_h160: format!("{:#x}", utxo.base_h160),
            slot_ranges: ranges
                .into_iter()
                .map(|range| SlotRangeResponse {
                    start: range.slot_start.to_string(),
                    end: range.slot_end.to_string(),
                })
                .collect(),
        });
    }

    assets.sort_by(|a, b| {
        a.collection_id
            .cmp(&b.collection_id)
            .then_with(|| a.init_owner_h160.cmp(&b.init_owner_h160))
    });

    Json(UtxoAssetsResponse {
        txid,
        vout,
        owner_h160,
        utxo_height,
        utxo_tx_index,
        assets,
    })
    .into_response()
}

pub async fn not_found() -> impl IntoResponse {
    json_error(StatusCode::NOT_FOUND, "endpoint not found")
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            message: message.to_string(),
        }),
    )
        .into_response()
}

fn internal_error() -> Response {
    json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
}

fn collection_to_response(collection: Collection) -> CollectionResponse {
    CollectionResponse {
        id: collection.key.to_string(),
        height: collection.key.block_height,
        tx_index: collection.key.tx_index,
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
            Block, Collection, CollectionKey, OwnershipRange, OwnershipRangeWithGroup,
            OwnershipUtxo, OwnershipUtxoSave, StorageRead, StorageTx, StorageWrite,
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
        assert_eq!(payload.height, collection.key.block_height);
        assert_eq!(payload.tx_index, collection.key.tx_index);
        assert_eq!(payload.token_id, token_decimal);
        assert!(matches!(
            payload.ownership_status,
            OwnershipStatus::InitialOwner
        ));
        assert_eq!(payload.owner_h160, expected_owner_h160);
        assert_eq!(payload.txid, None);
        assert_eq!(payload.vout, None);
        assert_eq!(payload.utxo_height, None);
        assert_eq!(payload.utxo_tx_index, None);
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

        let storage = TestStorage::with_collection(collection.clone()).with_ownership_utxo(
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
        assert_eq!(payload.height, collection.key.block_height);
        assert_eq!(payload.tx_index, collection.key.tx_index);
        assert_eq!(payload.token_id, token_decimal);
        assert!(matches!(
            payload.ownership_status,
            OwnershipStatus::RegisteredOwner
        ));
        assert_eq!(payload.owner_h160, format!("{:#x}", registered_owner));
        assert_eq!(payload.txid.as_deref(), Some("txid"));
        assert_eq!(payload.vout, Some(1));
        assert_eq!(payload.utxo_height, Some(840_001));
        assert_eq!(payload.utxo_tx_index, Some(2));
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

        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let payload: ErrorResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(payload.message, "collection not found");
    }

    #[tokio::test]
    async fn get_utxo_assets_returns_assets() {
        let collection = sample_collection();
        let token = sample_token();
        let reg_txid =
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();

        let utxo = OwnershipUtxo {
            collection_id: collection.key.clone(),
            reg_txid: reg_txid.clone(),
            reg_vout: 1,
            owner_h160: H160::from_low_u64_be(0x1234),
            base_h160: token.h160_address(),
            created_height: 840_001,
            created_tx_index: 2,
            spent_txid: None,
            spent_height: None,
            spent_tx_index: None,
        };

        let storage = TestStorage::with_collection(collection.clone()).with_ownership_utxo(
            utxo,
            vec![OwnershipRange {
                slot_start: token.slot_number(),
                slot_end: token.slot_number(),
            }],
        );

        let response = issue_utxo_request(storage, &reg_txid, "1").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let payload: UtxoAssetsResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(payload.txid, reg_txid);
        assert_eq!(payload.vout, 1);
        assert_eq!(
            payload.owner_h160,
            format!("{:#x}", H160::from_low_u64_be(0x1234))
        );
        assert_eq!(payload.utxo_height, 840_001);
        assert_eq!(payload.utxo_tx_index, 2);
        assert_eq!(payload.assets.len(), 1);
        assert_eq!(payload.assets[0].collection_id, collection.key.to_string());
        assert_eq!(
            payload.assets[0].init_owner_h160,
            format!("{:#x}", token.h160_address())
        );
        assert_eq!(payload.assets[0].slot_ranges.len(), 1);
        assert_eq!(
            payload.assets[0].slot_ranges[0].start,
            token.slot_number().to_string()
        );
        assert_eq!(
            payload.assets[0].slot_ranges[0].end,
            token.slot_number().to_string()
        );
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

    async fn issue_utxo_request(
        storage: TestStorage,
        txid: &str,
        vout: &str,
    ) -> axum::response::Response {
        let router = Router::new()
            .route(
                "/utxos/:txid/:vout/assets",
                get(get_utxo_assets::<TestStorage>),
            )
            .with_state(AppState {
                storage,
                started_at: SystemTime::now(),
            });

        router
            .oneshot(
                Request::builder()
                    .uri(format!("/utxos/{}/{}/assets", txid, vout))
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    type StoredOwnershipRange = (String, u32, CollectionKey, H160, OwnershipRange);

    #[derive(Clone, Default)]
    struct TestStorage {
        collections: Arc<RwLock<Vec<Collection>>>,
        ownership_utxos: Arc<RwLock<Vec<OwnershipUtxo>>>,
        ownership_ranges: Arc<RwLock<Vec<StoredOwnershipRange>>>,
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
                    guard.push((
                        utxo.reg_txid.clone(),
                        utxo.reg_vout,
                        utxo.collection_id.clone(),
                        utxo.base_h160,
                        range,
                    ));
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

        fn list_unspent_ownership_utxos_by_outpoint(
            &self,
            reg_txid: &str,
            reg_vout: u32,
        ) -> anyhow::Result<Vec<OwnershipUtxo>> {
            let utxos = self.ownership_utxos.read().unwrap();
            Ok(utxos
                .iter()
                .filter(|utxo| {
                    utxo.spent_txid.is_none()
                        && utxo.reg_txid == reg_txid
                        && utxo.reg_vout == reg_vout
                })
                .cloned()
                .collect())
        }

        fn list_unspent_ownership_ranges_by_outpoint(
            &self,
            reg_txid: &str,
            reg_vout: u32,
        ) -> anyhow::Result<Vec<OwnershipRangeWithGroup>> {
            let utxos = self.ownership_utxos.read().unwrap();
            let ranges = self.ownership_ranges.read().unwrap();

            Ok(ranges
                .iter()
                .filter(|(txid, vout, collection_id, base_h160, _)| {
                    *txid == reg_txid
                        && *vout == reg_vout
                        && utxos.iter().any(|utxo| {
                            utxo.spent_txid.is_none()
                                && utxo.reg_txid == *txid
                                && utxo.reg_vout == *vout
                                && utxo.collection_id == *collection_id
                                && utxo.base_h160 == *base_h160
                        })
                })
                .map(
                    |(_, _, collection_id, base_h160, range)| OwnershipRangeWithGroup {
                        collection_id: collection_id.clone(),
                        base_h160: *base_h160,
                        slot_start: range.slot_start,
                        slot_end: range.slot_end,
                    },
                )
                .collect())
        }

        fn list_ownership_ranges(
            &self,
            utxo: &OwnershipUtxo,
        ) -> anyhow::Result<Vec<OwnershipRange>> {
            let ranges = self.ownership_ranges.read().unwrap();
            Ok(ranges
                .iter()
                .filter(|(txid, vout, collection_id, base_h160, _)| {
                    txid == &utxo.reg_txid
                        && *vout == utxo.reg_vout
                        && collection_id == &utxo.collection_id
                        && *base_h160 == utxo.base_h160
                })
                .map(|(_, _, _, _, range)| range.clone())
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
                let covers = ranges
                    .iter()
                    .any(|(txid, vout, collection_id, base_h160, range)| {
                        txid == &utxo.reg_txid
                            && *vout == utxo.reg_vout
                            && collection_id == &utxo.collection_id
                            && *base_h160 == utxo.base_h160
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

        fn list_unspent_ownership_utxos_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> anyhow::Result<Vec<OwnershipUtxo>> {
            Err(anyhow!("not implemented"))
        }

        fn list_unspent_ownership_ranges_by_outpoint(
            &self,
            _reg_txid: &str,
            _reg_vout: u32,
        ) -> anyhow::Result<Vec<OwnershipRangeWithGroup>> {
            Err(anyhow!("not implemented"))
        }

        fn list_ownership_ranges(
            &self,
            _utxo: &OwnershipUtxo,
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
            _collection_id: &CollectionKey,
            _base_h160: H160,
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
