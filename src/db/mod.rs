// Keep repository traits and implementations organized here.
pub mod repository;
pub mod sqlite;

// Re-export the public interface for downstream consumers.
pub use repository::{CollectionRow, Repository};
pub use sqlite::SqliteRepo;
