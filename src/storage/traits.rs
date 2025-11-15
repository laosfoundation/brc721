#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionKey {
    pub block_height: u64,
    pub tx_index: u32,
}

pub trait Storage {
    fn load_last(&self) -> anyhow::Result<Option<Block>>;
    fn save_last(&self, height: u64, hash: &str) -> anyhow::Result<()>;
    fn save_collection(
        &self,
        key: CollectionKey,
        owner: String,
        params: String,
    ) -> anyhow::Result<()>;
    fn list_collections(&self) -> anyhow::Result<Vec<(CollectionKey, String, String)>>;
}
