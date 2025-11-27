#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionKey {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection {
    pub key: CollectionKey,
    pub evm_collection_address: String,
    pub rebaseable: bool,
}
