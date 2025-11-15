#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionKey {
    pub block_height: u64,
    pub txid: String,
}

pub trait Storage {
    fn load_last(&self) -> anyhow::Result<Option<Block>>;
    fn save_last(&self, height: u64, hash: &str) -> anyhow::Result<()>;
}
