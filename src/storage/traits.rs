use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastBlock {
    pub height: u64,
    pub hash: String,
}

pub type CollectionRow = (String, [u8; 20], bool, u64, String, u32);

pub trait Storage {
    fn load_last(&self) -> io::Result<Option<LastBlock>>;
    fn save_last(&self, height: u64, hash: &str) -> io::Result<()>;
    fn insert_collections_batch(&self, rows: &[CollectionRow]) -> rusqlite::Result<()>;
}
