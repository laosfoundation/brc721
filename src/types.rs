use ethereum_types::H160;

pub type CollectionAddress = H160;

pub const REGISTER_COLLECTION_FLAG: u8 = 0x00;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterCollectionPayload {
    /// The 160-bit EVM address of the collection's smart contract.
    pub collection_address: CollectionAddress,

    /// A boolean indicating whether the collection supports future Rebase transactions.
    pub rebaseable: bool,
}
