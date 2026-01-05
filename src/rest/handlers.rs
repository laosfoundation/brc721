use std::{collections::HashMap, fmt, str::FromStr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use bitcoin::{
    hashes::{hash160, Hash as _},
    Address,
};
use ethereum_types::{H160, U256};

use crate::{
    storage::{
        traits::{Collection, CollectionKey, OwnershipRange},
        Storage,
    },
    types::{Brc721Error, Brc721Token},
};

use super::{
    models::{
        AddressAssetsResponse, AddressesAssetsRequest, AddressesAssetsResponse, ChainStateResponse,
        CollectionResponse, CollectionsResponse, ErrorResponse, HealthResponse, LastBlock,
        OwnershipStatus, OwnershipUtxoResponse, SlotRangeResponse, TokenOwnerResponse,
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

pub async fn get_address_assets<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    let checked =
        match Address::from_str(&address).and_then(|addr| addr.require_network(state.network)) {
            Ok(addr) => addr,
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

    let owner_h160 = owner_h160_for_script(&checked.script_pubkey());
    let ranges = match state.storage.list_unspent_ownership_by_owner(owner_h160) {
        Ok(ranges) => ranges,
        Err(err) => {
            log::error!("Failed to load assets for address {}: {:?}", address, err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    Json(AddressAssetsResponse {
        address,
        owner_h160: format!("{:#x}", owner_h160),
        utxos: group_ownership_ranges(ranges),
    })
    .into_response()
}

pub async fn post_addresses_assets<S: Storage + Clone + Send + Sync + 'static>(
    State(state): State<AppState<S>>,
    Json(req): Json<AddressesAssetsRequest>,
) -> impl IntoResponse {
    const MAX_ADDRESSES: usize = 5_000;
    if req.addresses.len() > MAX_ADDRESSES {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                message: format!("too many addresses (max {MAX_ADDRESSES})"),
            }),
        )
            .into_response();
    }

    let mut requested = Vec::with_capacity(req.addresses.len());
    for address in req.addresses {
        let checked = match Address::from_str(&address)
            .and_then(|addr| addr.require_network(state.network))
        {
            Ok(addr) => addr,
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

        let owner_h160 = owner_h160_for_script(&checked.script_pubkey());
        requested.push((address, owner_h160));
    }

    let owners = requested
        .iter()
        .map(|(_, owner_h160)| *owner_h160)
        .collect::<Vec<_>>();

    let all_ranges = match state.storage.list_unspent_ownership_by_owners(&owners) {
        Ok(ranges) => ranges,
        Err(err) => {
            log::error!("Failed to load assets for bulk request: {:?}", err);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut by_owner: HashMap<H160, Vec<OwnershipRange>> = HashMap::new();
    for range in all_ranges {
        by_owner.entry(range.owner_h160).or_default().push(range);
    }

    let mut results = Vec::with_capacity(requested.len());
    for (address, owner_h160) in requested {
        let ranges = by_owner.remove(&owner_h160).unwrap_or_default();
        results.push(AddressAssetsResponse {
            address,
            owner_h160: format!("{:#x}", owner_h160),
            utxos: group_ownership_ranges(ranges),
        });
    }

    Json(AddressesAssetsResponse { results }).into_response()
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

    let owner_h160 = format_owner_h160(&token);

    Json(TokenOwnerResponse {
        collection_id: key.to_string(),
        token_id: format_token_id(&token),
        ownership_status: OwnershipStatus::InitialOwner,
        owner_h160,
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

fn owner_h160_for_script(script: &bitcoin::ScriptBuf) -> H160 {
    let script_hash = hash160::Hash::hash(script.as_bytes());
    H160::from_slice(script_hash.as_byte_array())
}

fn group_ownership_ranges(ranges: Vec<OwnershipRange>) -> Vec<OwnershipUtxoResponse> {
    let mut out = Vec::new();
    let mut current_key: Option<(String, bitcoin::Txid, u32)> = None;
    let mut current: Option<OwnershipUtxoResponse> = None;

    for range in ranges {
        let key = (
            range.collection_id.to_string(),
            range.outpoint.txid,
            range.outpoint.vout,
        );
        if current_key.as_ref() != Some(&key) {
            if let Some(previous) = current.take() {
                out.push(previous);
            }

            current_key = Some(key.clone());
            current = Some(OwnershipUtxoResponse {
                collection_id: key.0.clone(),
                txid: key.1.to_string(),
                vout: key.2,
                created_height: range.created_height,
                created_tx_index: range.created_tx_index,
                slot_ranges: Vec::new(),
            });
        }

        if let Some(current) = current.as_mut() {
            current.slot_ranges.push(SlotRangeResponse {
                start: range.slot_start.to_string(),
                end: range.slot_end.to_string(),
            });
        }
    }

    if let Some(last) = current {
        out.push(last);
    }

    out
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
            Block, Collection, CollectionKey, OwnershipRange, StorageRead, StorageTx, StorageWrite,
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
        assert_eq!(payload.owner_h160, expected_owner_h160);
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

    #[tokio::test]
    async fn get_address_assets_returns_grouped_utxos() {
        use bitcoin::hashes::hash160;
        use bitcoin::hashes::Hash as _;
        use bitcoin::{Address, Network, OutPoint};
        use std::str::FromStr;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let storage = crate::storage::SqliteStorage::new(temp_dir.path().join("brc721_rest.db"));
        storage.init().expect("init storage");

        let address =
            Address::from_str("bcrt1p8wpt9v4frpf3tkn0srd97pksgsxc5hs52lafxwru9kgeephvs7rqjeprhg")
                .unwrap()
                .require_network(Network::Regtest)
                .unwrap();
        let script = address.script_pubkey();
        let script_hash = hash160::Hash::hash(script.as_bytes());
        let owner_h160 = H160::from_slice(script_hash.as_byte_array());

        let outpoint = OutPoint {
            txid: bitcoin::Txid::all_zeros(),
            vout: 1,
        };

        let tx = storage.begin_tx().unwrap();
        tx.insert_ownership_range(CollectionKey::new(1, 0), owner_h160, outpoint, 0, 9, 100, 2)
            .unwrap();
        tx.insert_ownership_range(
            CollectionKey::new(1, 0),
            owner_h160,
            outpoint,
            42,
            42,
            100,
            2,
        )
        .unwrap();
        tx.commit().unwrap();

        let router = Router::new()
            .route(
                "/addresses/:address/assets",
                get(get_address_assets::<crate::storage::SqliteStorage>),
            )
            .with_state(AppState {
                storage: storage.clone(),
                network: bitcoin::Network::Regtest,
                started_at: SystemTime::now(),
            });

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/addresses/{}/assets", address))
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let payload: AddressAssetsResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(payload.address, address.to_string());
        assert_eq!(payload.owner_h160, format!("{:#x}", owner_h160));
        assert_eq!(payload.utxos.len(), 1);
        assert_eq!(payload.utxos[0].collection_id, "1:0");
        assert_eq!(
            payload.utxos[0].txid,
            bitcoin::Txid::all_zeros().to_string()
        );
        assert_eq!(payload.utxos[0].vout, 1);
        assert_eq!(payload.utxos[0].slot_ranges.len(), 2);
        assert_eq!(payload.utxos[0].slot_ranges[0].start, "0");
        assert_eq!(payload.utxos[0].slot_ranges[0].end, "9");
        assert_eq!(payload.utxos[0].slot_ranges[1].start, "42");
        assert_eq!(payload.utxos[0].slot_ranges[1].end, "42");
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
                network: bitcoin::Network::Regtest,
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

        fn has_unspent_slot_overlap(
            &self,
            _collection_id: &CollectionKey,
            _slot_start: u128,
            _slot_end: u128,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }

        fn list_unspent_ownership_by_owner(
            &self,
            _owner_h160: H160,
        ) -> anyhow::Result<Vec<OwnershipRange>> {
            Ok(Vec::new())
        }

        fn list_unspent_ownership_by_owners(
            &self,
            _owner_h160s: &[H160],
        ) -> anyhow::Result<Vec<OwnershipRange>> {
            Ok(Vec::new())
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

        fn has_unspent_slot_overlap(
            &self,
            _collection_id: &CollectionKey,
            _slot_start: u128,
            _slot_end: u128,
        ) -> anyhow::Result<bool> {
            Err(anyhow!("not implemented"))
        }

        fn list_unspent_ownership_by_owner(
            &self,
            _owner_h160: H160,
        ) -> anyhow::Result<Vec<OwnershipRange>> {
            Err(anyhow!("not implemented"))
        }

        fn list_unspent_ownership_by_owners(
            &self,
            _owner_h160s: &[H160],
        ) -> anyhow::Result<Vec<OwnershipRange>> {
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

        fn insert_ownership_range(
            &self,
            _collection_id: CollectionKey,
            _owner_h160: H160,
            _outpoint: bitcoin::OutPoint,
            _slot_start: u128,
            _slot_end: u128,
            _created_height: u64,
            _created_tx_index: u32,
        ) -> anyhow::Result<()> {
            Err(anyhow!("not implemented"))
        }

        fn mark_ownership_outpoint_spent(
            &self,
            _outpoint: bitcoin::OutPoint,
            _spent_height: u64,
            _spent_txid: bitcoin::Txid,
        ) -> anyhow::Result<usize> {
            Err(anyhow!("not implemented"))
        }
    }

    impl StorageTx for NoopTx {
        fn commit(self) -> anyhow::Result<()> {
            Ok(())
        }
    }
}
