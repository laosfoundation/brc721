use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::{Block, Storage};
use crate::storage::traits::CollectionKey;

const DB_SCHEMA_VERSION: i64 = 1;

#[derive(Clone)]
pub struct SqliteStorage {
    pub path: String,
}

impl SqliteStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_string_lossy().to_string(),
        }
    }

    pub fn reset_all(&self) -> anyhow::Result<()> {
        if !std::path::Path::new(&self.path).exists() {
            return Ok(());
        }
        std::fs::remove_file(&self.path)?;
        Ok(())
    }

    pub fn init(&self) -> anyhow::Result<()> {
        self.with_conn(|_conn| Ok(()))?;
        Ok(())
    }

    fn with_conn<F, T>(&self, f: F) -> rusqlite::Result<T>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<T>,
    {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_millis(500))?;

        Self::migrate(&conn)?;
        f(&conn)
    }

    fn migrate(conn: &Connection) -> rusqlite::Result<()> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version == DB_SCHEMA_VERSION {
            return Ok(());
        }

        if version == 0 {
            conn.execute_batch(
                r#"
            CREATE TABLE chain_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                height INTEGER NOT NULL,
                hash TEXT NOT NULL
            );
            CREATE TABLE collections (
                id TEXT PRIMARY KEY,
                evm_collection_address TEXT NOT NULL,
                rebaseable INTEGER NOT NULL
            );
        "#,
            )?;
            conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            return Ok(());
        }

        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::SchemaChanged as i32),
            Some("database schema version mismatch; please run with --reset option".to_string()),
        ))
    }
}

impl Storage for SqliteStorage {
    fn load_last(&self) -> anyhow::Result<Option<Block>> {
        let opt = self.with_conn(|conn| {
            conn.query_row(
                "SELECT height, hash FROM chain_state WHERE id = 1",
                [],
                |row| {
                    let height: i64 = row.get(0)?;
                    let hash: String = row.get(1)?;
                    Ok(Block {
                        height: height as u64,
                        hash,
                    })
                },
            )
            .optional()
        })?;
        Ok(opt)
    }

    fn save_last(&self, height: u64, hash: &str) -> anyhow::Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO chain_state (id, height, hash) VALUES (1, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET height=excluded.height, hash=excluded.hash",
                params![height as i64, hash],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    fn save_collection(
        &self,
        key: CollectionKey,
        evm_collection_address: String,
        rebaseable: bool,
    ) -> anyhow::Result<()> {
        let id = key.id;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO collections (id, evm_collection_address, rebaseable) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET evm_collection_address=excluded.evm_collection_address, rebaseable=excluded.rebaseable",
                params![id, evm_collection_address, rebaseable as i64],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    fn list_collections(&self) -> anyhow::Result<Vec<(CollectionKey, String, bool)>> {
        let rows = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, evm_collection_address, rebaseable FROM collections ORDER BY id",
            )?;
            let mapped = stmt
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let evm_collection_address: String = row.get(1)?;
                    let rebaseable_int: i64 = row.get(2)?;
                    let rebaseable = rebaseable_int != 0;
                    Ok((CollectionKey { id }, evm_collection_address, rebaseable))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(mapped)
        })?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::DB_SCHEMA_VERSION;
    use rusqlite::{Connection, OptionalExtension};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_file(prefix: &str, ext: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("{}_{}.{}", prefix, nanos, ext));
        p
    }

    #[test]
    fn sqlite_reset_all_ok_when_missing() {
        let path = unique_temp_file("brc721_reset", "db");
        let repo = SqliteStorage::new(&path);
        repo.reset_all().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn sqlite_reset_all_removes_existing_file() {
        let path = unique_temp_file("brc721_reset", "db");
        std::fs::write(&path, b"dummy").unwrap();
        assert!(path.exists());
        let repo = SqliteStorage::new(&path);
        repo.reset_all().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn sqlite_init_initializes_schema() {
        let path = unique_temp_file("brc721_init", "db");
        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        assert!(path.exists());

        let conn = Connection::open(&path).unwrap();
        let chain_state = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='chain_state'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap();
        assert_eq!(chain_state.as_deref(), Some("chain_state"));

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_SCHEMA_VERSION);
    }

    #[test]
    fn sqlite_init_installs_schema_on_existing_vanilla_db() {
        let path = unique_temp_file("brc721_init_existing", "db");
        std::fs::File::create(&path).unwrap();
        assert!(path.exists());

        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_SCHEMA_VERSION);
    }

    #[test]
    fn sqlite_fails_on_mismatched_schema_version() {
        let path = unique_temp_file("brc721_bad_version", "db");
        let repo = SqliteStorage::new(&path);

        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            PRAGMA user_version = 999;
            "#,
        )
        .unwrap();

        let err = repo
            .init()
            .expect_err("init should fail on version mismatch");
        let msg = format!("{err}");
        assert!(msg.contains("database schema version mismatch"));
        assert!(msg.contains("--reset"));
    }

    #[test]
    fn sqlite_save_last_inserts_then_updates() {
        let path = unique_temp_file("brc721_save_last", "db");
        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        assert_eq!(repo.load_last().unwrap(), None);

        repo.save_last(100, "hash100").unwrap();
        let first = repo.load_last().unwrap().unwrap();
        assert_eq!(first.height, 100);
        assert_eq!(first.hash, "hash100");

        repo.save_last(101, "hash101").unwrap();
        let second = repo.load_last().unwrap().unwrap();
        assert_eq!(second.height, 101);
        assert_eq!(second.hash, "hash101");

        let conn = Connection::open(&path).unwrap();
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chain_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, 1);
    }

    #[test]
    fn sqlite_save_and_list_collections_persists_data() {
        let path = unique_temp_file("brc721_save_collection", "db");
        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        repo.save_collection(
            CollectionKey {
                id: "123:0".to_string(),
            },
            "0xaaaa".to_string(),
            true,
        )
        .unwrap();
        repo.save_collection(
            CollectionKey {
                id: "124:1".to_string(),
            },
            "0xbbbb".to_string(),
            false,
        )
        .unwrap();

        let collections = repo.list_collections().unwrap();
        assert_eq!(collections.len(), 2);
        assert_eq!(collections[0].0.id, "123:0");
        assert_eq!(collections[0].1, "0xaaaa");
        assert!(collections[0].2);
        assert_eq!(collections[1].0.id, "124:1");
        assert_eq!(collections[1].1, "0xbbbb");
        assert!(!collections[1].2);
    }
}
