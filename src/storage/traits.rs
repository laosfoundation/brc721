use anyhow::Result;
use ethereum_types::H160;

pub use super::collection::{Collection, CollectionKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnershipUtxo {
    pub collection_id: CollectionKey,
    pub reg_txid: String,
    pub reg_vout: u32,
    pub owner_h160: H160,
    pub base_h160: H160,
    pub created_height: u64,
    pub created_tx_index: u32,
    pub spent_txid: Option<String>,
    pub spent_height: Option<u64>,
    pub spent_tx_index: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnershipUtxoSave<'a> {
    pub collection_id: &'a CollectionKey,
    pub owner_h160: H160,
    pub base_h160: H160,
    pub reg_txid: &'a str,
    pub reg_vout: u32,
    pub created_height: u64,
    pub created_tx_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnershipRange {
    pub slot_start: u128,
    pub slot_end: u128,
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
    fn list_unspent_ownership_utxos_by_outpoint(
        &self,
        reg_txid: &str,
        reg_vout: u32,
    ) -> Result<Vec<OwnershipUtxo>>;
    fn list_ownership_ranges(&self, utxo: &OwnershipUtxo) -> Result<Vec<OwnershipRange>>;
    fn find_unspent_ownership_utxo_for_slot(
        &self,
        collection_id: &CollectionKey,
        base_h160: H160,
        slot: u128,
    ) -> Result<Option<OwnershipUtxo>>;
    fn list_unspent_ownership_utxos_by_owner(&self, owner_h160: H160)
        -> Result<Vec<OwnershipUtxo>>;
}

pub trait StorageWrite {
    fn save_last(&self, height: u64, hash: &str) -> Result<()>;
    fn save_collection(
        &self,
        key: CollectionKey,
        evm_collection_address: H160,
        rebaseable: bool,
    ) -> Result<()>;
    fn save_ownership_utxo(&self, utxo: OwnershipUtxoSave<'_>) -> Result<()>;
    fn save_ownership_range(
        &self,
        reg_txid: &str,
        reg_vout: u32,
        collection_id: &CollectionKey,
        base_h160: H160,
        slot_start: u128,
        slot_end: u128,
    ) -> Result<()>;
    fn mark_ownership_utxo_spent(
        &self,
        reg_txid: &str,
        reg_vout: u32,
        spent_txid: &str,
        spent_height: u64,
        spent_tx_index: u32,
    ) -> Result<()>;
}

pub trait StorageTx: StorageRead + StorageWrite {
    fn commit(self) -> Result<()>;
}

pub trait Storage: StorageRead {
    type Tx: StorageTx;

    fn begin_tx(&self) -> Result<Self::Tx>;
}
