use bitcoin::OutPoint;

use super::collection::CollectionKey;
use crate::types::TokenId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenKey {
    pub collection: CollectionKey,
    pub token_id: TokenId,
}

impl TokenKey {
    pub fn new(collection: CollectionKey, token_id: TokenId) -> Self {
        Self {
            collection,
            token_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenOwnership {
    pub key: TokenKey,
    pub owner_outpoint: OutPoint,
    pub registered_block_height: u64,
    pub registered_tx_index: u32,
}
