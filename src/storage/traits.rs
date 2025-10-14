use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub hash: String,
}

pub trait Storage {
    fn load_last(&self) -> io::Result<Option<Block>>;
    fn save_last(&self, height: u64, hash: &str) -> io::Result<()>;
}
