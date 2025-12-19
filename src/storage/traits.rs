use anyhow::Result;
use bitcoin::OutPoint;
use ethereum_types::H160;

pub use super::collection::{Collection, CollectionKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnershipRange {
    pub collection: CollectionKey,
    pub initial_owner_h160: H160,
    pub slot_start: u128,
    pub slot_end: u128,
    pub owner_h160: H160,
    pub outpoint: OutPoint,
}

pub trait StorageRead {
    fn load_last(&self) -> Result<Option<Block>>;
    fn load_collection(&self, id: &CollectionKey) -> Result<Option<Collection>>;
    fn list_collections(&self) -> Result<Vec<Collection>>;

    fn has_ownership_overlap(
        &self,
        _collection: &CollectionKey,
        _initial_owner_h160: H160,
        _slot_start: u128,
        _slot_end: u128,
    ) -> Result<bool> {
        Ok(false)
    }

    fn load_registered_owner_h160(
        &self,
        _collection: &CollectionKey,
        _token_id: &crate::types::Brc721Token,
    ) -> Result<Option<H160>> {
        Ok(None)
    }
}

pub trait StorageWrite {
    fn save_last(&self, height: u64, hash: &str) -> Result<()>;
    fn save_collection(
        &self,
        key: CollectionKey,
        evm_collection_address: H160,
        rebaseable: bool,
    ) -> Result<()>;

    fn save_ownership_ranges(&self, _ranges: &[OwnershipRange]) -> Result<()> {
        Ok(())
    }
}

pub trait StorageTx: StorageRead + StorageWrite {
    fn commit(self) -> Result<()>;
}

pub trait Storage: StorageRead {
    type Tx: StorageTx;

    fn begin_tx(&self) -> Result<Self::Tx>;
}
