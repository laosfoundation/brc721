use anyhow::Result;
use ethereum_types::H160;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionKey {
    pub id: String,
}

pub trait StorageRead {
    fn load_last(&self) -> Result<Option<Block>>;
    fn list_collections(&self) -> Result<Vec<(CollectionKey, String, bool)>>;
}

pub trait StorageWrite {
    fn save_last(&self, height: u64, hash: &str) -> Result<()>;
    fn save_collection(
        &self,
        key: CollectionKey,
        evm_collection_address: H160,
        rebaseable: bool,
    ) -> Result<()>;
}

pub trait StorageTx: StorageRead + StorageWrite {
    #[allow(dead_code)]
    fn commit(self) -> Result<()>;
    #[allow(dead_code)]
    fn rollback(self) -> Result<()>;
}

pub trait Storage: StorageRead {
    type Tx: StorageTx;

    #[allow(dead_code)]
    fn begin_tx(&self) -> Result<Self::Tx>;
}
