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
    pub owner_h160: H160,
    pub collection_id: CollectionKey,
    pub outpoint: OutPoint,
    pub slot_start: u128,
    pub slot_end: u128,
    pub created_height: u64,
    pub created_tx_index: u32,
}

pub trait StorageRead {
    fn load_last(&self) -> Result<Option<Block>>;
    fn load_collection(&self, id: &CollectionKey) -> Result<Option<Collection>>;
    fn list_collections(&self) -> Result<Vec<Collection>>;

    fn has_unspent_slot_overlap(
        &self,
        collection_id: &CollectionKey,
        slot_start: u128,
        slot_end: u128,
    ) -> Result<bool>;

    fn list_unspent_ownership_by_owner(&self, owner_h160: H160) -> Result<Vec<OwnershipRange>>;
    fn list_unspent_ownership_by_owners(&self, owner_h160: &[H160]) -> Result<Vec<OwnershipRange>>;
}

pub trait StorageWrite {
    fn save_last(&self, height: u64, hash: &str) -> Result<()>;
    fn save_collection(
        &self,
        key: CollectionKey,
        evm_collection_address: H160,
        rebaseable: bool,
    ) -> Result<()>;

    fn insert_ownership_range(
        &self,
        collection_id: CollectionKey,
        owner_h160: H160,
        outpoint: OutPoint,
        slot_start: u128,
        slot_end: u128,
        created_height: u64,
        created_tx_index: u32,
    ) -> Result<()>;

    /// Marks all ownership ranges associated with the given output as spent.
    ///
    /// Returns the number of rows updated.
    fn mark_ownership_outpoint_spent(
        &self,
        outpoint: OutPoint,
        spent_height: u64,
        spent_txid: bitcoin::Txid,
    ) -> Result<usize>;
}

pub trait StorageTx: StorageRead + StorageWrite {
    fn commit(self) -> Result<()>;
}

pub trait Storage: StorageRead {
    type Tx: StorageTx;

    fn begin_tx(&self) -> Result<Self::Tx>;
}
