use anyhow::Result;
use ethereum_types::H160;

pub use super::collection::{Collection, CollectionKey};
pub use super::token::{TokenKey, TokenOwnership};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub hash: String,
}

pub trait StorageRead {
    fn load_last(&self) -> Result<Option<Block>>;
    fn load_collection(&self, id: &CollectionKey) -> Result<Option<Collection>>;
    fn list_collections(&self) -> Result<Vec<Collection>>;
    fn load_token(&self, key: &TokenKey) -> Result<Option<TokenOwnership>>;
}

pub trait StorageWrite {
    fn save_last(&self, height: u64, hash: &str) -> Result<()>;
    fn save_collection(
        &self,
        key: CollectionKey,
        evm_collection_address: H160,
        rebaseable: bool,
    ) -> Result<()>;
    fn save_token(&self, token: &TokenOwnership) -> Result<()>;
}

pub trait StorageTx: StorageRead + StorageWrite {
    fn commit(self) -> Result<()>;
}

pub trait Storage: StorageRead {
    type Tx: StorageTx;

    fn begin_tx(&self) -> Result<Self::Tx>;
}
