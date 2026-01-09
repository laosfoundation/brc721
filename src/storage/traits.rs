use anyhow::Result;
use ethereum_types::H160;

pub use super::collection::{Collection, CollectionKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredToken {
    pub collection_id: CollectionKey,
    pub token_id: String,
    pub owner_h160: H160,
    pub reg_txid: String,
    pub reg_vout: u32,
    pub created_height: u64,
    pub created_tx_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisteredTokenSave<'a> {
    pub collection_id: &'a CollectionKey,
    pub token_id: &'a str,
    pub owner_h160: H160,
    pub reg_txid: &'a str,
    pub reg_vout: u32,
    pub created_height: u64,
    pub created_tx_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub hash: String,
}

pub trait StorageRead {
    fn load_last(&self) -> Result<Option<Block>>;
    fn load_collection(&self, id: &CollectionKey) -> Result<Option<Collection>>;
    fn list_collections(&self) -> Result<Vec<Collection>>;
    fn load_registered_token(
        &self,
        collection_id: &CollectionKey,
        token_id: &str,
    ) -> Result<Option<RegisteredToken>>;
}

pub trait StorageWrite {
    fn save_last(&self, height: u64, hash: &str) -> Result<()>;
    fn save_collection(
        &self,
        key: CollectionKey,
        evm_collection_address: H160,
        rebaseable: bool,
    ) -> Result<()>;
    fn save_registered_token(&self, token: RegisteredTokenSave<'_>) -> Result<()>;
}

pub trait StorageTx: StorageRead + StorageWrite {
    fn commit(self) -> Result<()>;
}

pub trait Storage: StorageRead {
    type Tx: StorageTx;

    fn begin_tx(&self) -> Result<Self::Tx>;
}
