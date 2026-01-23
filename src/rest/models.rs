use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_secs: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainStateResponse {
    pub last: Option<LastBlock>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResponse {
    pub id: String,
    pub height: u64,
    pub tx_index: u32,
    pub evm_collection_address: String,
    pub rebaseable: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionsResponse {
    pub collections: Vec<CollectionResponse>,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub message: String,
}

#[derive(Serialize, Deserialize)]
pub struct LastBlock {
    pub height: u64,
    pub hash: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenOwnerResponse {
    pub collection_id: String,
    pub height: u64,
    pub tx_index: u32,
    pub token_id: String,
    pub ownership_status: OwnershipStatus,
    pub owner_h160: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vout: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utxo_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utxo_tx_index: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OwnershipStatus {
    InitialOwner,
    RegisteredOwner,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressAssetsResponse {
    pub address: String,
    pub address_h160: String,
    pub utxos: Vec<OwnershipUtxoResponse>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UtxoAssetsResponse {
    pub txid: String,
    pub vout: u32,
    pub owner_h160: String,
    pub utxo_height: u64,
    pub utxo_tx_index: u32,
    pub assets: Vec<UtxoOwnershipResponse>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UtxoOwnershipResponse {
    pub collection_id: String,
    pub init_owner_h160: String,
    pub slot_ranges: Vec<SlotRangeResponse>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipUtxoResponse {
    pub collection_id: String,
    pub txid: String,
    pub vout: u32,
    pub init_owner_h160: String,
    pub utxo_height: u64,
    pub utxo_tx_index: u32,
    pub slot_ranges: Vec<SlotRangeResponse>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotRangeResponse {
    pub start: String,
    pub end: String,
}
