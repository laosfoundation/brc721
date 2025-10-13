// Shared repository contract and row shape.
use crate::storage::Storage;

pub type CollectionRow = (String, [u8; 20], bool, u64, String, u32);

pub trait Repository: Storage {
    /// Insert many collection rows atomically.
    fn insert_collections_batch(&self, rows: &[CollectionRow]) -> rusqlite::Result<()>;
}
