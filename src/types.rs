use ethereum_types::H160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterCollectionPayload {
    /// The 160-bit EVM address of the collection's smart contract.
    pub laos_collection_address: H160,

    /// A boolean indicating whether the collection supports future Rebase transactions.
    pub rebaseable: bool,
}
