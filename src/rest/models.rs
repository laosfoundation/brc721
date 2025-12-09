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
    pub token_id: String,
    pub ownership_status: OwnershipStatus,
    pub owner_h160: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OwnershipStatus {
    InitialOwner,
    #[allow(dead_code)]
    RegisteredOwner,
}
