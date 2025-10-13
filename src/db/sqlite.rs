// SQLite-backed repository implementation.
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::storage::{LastBlock, Storage};

use super::{CollectionRow, Repository};

#[derive(Clone)]
pub struct SqliteRepo {
    pub path: String,
}

impl SqliteRepo {
    /// Build a repository that targets the provided SQLite database path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_string_lossy().to_string(),
        }
    }

    /// Remove the backing database file to force a clean start.
    pub fn reset_all(&self) -> std::io::Result<()> {
        if !std::path::Path::new(&self.path).exists() {
            return Ok(());
        }
        std::fs::remove_file(&self.path)
    }

    /// Perform any one-off database import before normal use.
    pub fn import_if_needed(&self) -> std::io::Result<()> {
        self.with_conn(|_conn| {
            // No legacy import in dev mode
            Ok(())
        })
        .map_err(|e| std::io::Error::other(e.to_string()))
    }

    /// Open a connection, ensure schema, and run the supplied closure.
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

    /// Create missing tables and indexes.
    fn migrate(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS chain_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                height INTEGER NOT NULL,
                hash TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS collections (
                id TEXT PRIMARY KEY,
                laos_addr BLOB NOT NULL,
                rebaseable INTEGER NOT NULL,
                block_height INTEGER NOT NULL,
                block_hash TEXT NOT NULL,
                tx_index INTEGER NOT NULL,
                inserted_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_collections_laos ON collections(laos_addr);
            CREATE INDEX IF NOT EXISTS idx_collections_height ON collections(block_height);
            "#,
        )
    }

    /// Return the current UNIX timestamp in seconds.
    fn now_ts() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }
}

impl Storage for SqliteRepo {
    /// Fetch the most recent chain state from persistent storage.
    fn load_last(&self) -> std::io::Result<Option<LastBlock>> {
        let r = self.with_conn(|conn| {
            conn.query_row(
                "SELECT height, hash FROM chain_state WHERE id = 1",
                [],
                |row| {
                    let height: i64 = row.get(0)?;
                    let hash: String = row.get(1)?;
                    Ok(LastBlock {
                        height: height as u64,
                        hash,
                    })
                },
            )
            .optional()
        });
        match r {
            Ok(opt) => Ok(opt),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    }

    /// Upsert the current chain state.
    fn save_last(&self, height: u64, hash: &str) -> std::io::Result<()> {
        let r = self.with_conn(|conn| {
            let ts = Self::now_ts();
            conn.execute(
                "INSERT INTO chain_state (id, height, hash, updated_at) VALUES (1, ?, ?, ?)\n                 ON CONFLICT(id) DO UPDATE SET height=excluded.height, hash=excluded.hash, updated_at=excluded.updated_at",
                params![height as i64, hash, ts],
            )?;
            Ok(())
        });
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    }
}

impl Repository for SqliteRepo {
    /// Insert collection entries while preserving idempotency and batching.
    fn insert_collections_batch(&self, rows: &[CollectionRow]) -> rusqlite::Result<()> {
        self.with_conn(|conn| {
            let ts = Self::now_ts();
            let tx = conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR IGNORE INTO collections (id, laos_addr, rebaseable, block_height, block_hash, tx_index, inserted_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
                )?;
                for (id, laos, rebaseable, height, hash, tx_index) in rows.iter() {
                    stmt.execute(params![id, &laos[..], if *rebaseable {1} else {0}, *height as i64, hash, *tx_index as i64, ts])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Generate a unique path in the temp directory.
    fn unique_temp_path() -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("brc721_reset_{}.db", nanos));
        p
    }

    /// reset_all returns Ok when the file does not exist.
    #[test]
    fn reset_all_ok_when_missing() {
        let path = unique_temp_path();
        let repo = SqliteRepo::new(&path);
        repo.reset_all().unwrap();
        assert!(!path.exists());
    }

    /// reset_all removes the database file when present.
    #[test]
    fn reset_all_removes_existing_file() {
        let path = unique_temp_path();
        std::fs::write(&path, b"dummy").unwrap();
        assert!(path.exists());
        let repo = SqliteRepo::new(&path);
        repo.reset_all().unwrap();
        assert!(!path.exists());
    }
}
