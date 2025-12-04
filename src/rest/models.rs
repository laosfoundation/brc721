use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_secs: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainStateResponse {
    pub last: Option<LastBlock>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResponse {
    pub id: String,
    pub evm_collection_address: String,
    pub rebaseable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionsResponse {
    pub collections: Vec<CollectionResponse>,
}

#[derive(Serialize)]
pub struct LastBlock {
    pub height: u64,
    pub hash: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenOwnerResponse {
    pub collection_id: String,
    pub token_id: String,
    pub ownership_status: OwnershipStatus,
    pub owner: TokenOwnerDetails,
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OwnershipStatus {
    InitialOwner,
    #[allow(dead_code)]
    RegisteredOwner,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TokenOwnerDetails {
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
