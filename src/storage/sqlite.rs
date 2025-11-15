use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::{Block, Storage};
use crate::storage::traits::CollectionKey;

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
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS chain_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                height INTEGER NOT NULL,
                hash TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS collections (
                block_height INTEGER NOT NULL,
                txid TEXT NOT NULL,
                owner TEXT NOT NULL,
                params TEXT NOT NULL,
                PRIMARY KEY (block_height, txid)
            );
            "#,
        )
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

    fn save_collection(&self, key: CollectionKey, owner: String, params: String) -> anyhow::Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO collections (block_height, txid, owner, params) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(block_height, txid) DO UPDATE SET owner=excluded.owner, params=excluded.params",
                params![key.block_height as i64, key.txid, owner, params],
            )?;
            Ok(())
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
