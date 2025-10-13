use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use super::{Block, CollectionRow, Storage};

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

    pub fn reset_all(&self) -> std::io::Result<()> {
        if !std::path::Path::new(&self.path).exists() {
            return Ok(());
        }
        std::fs::remove_file(&self.path)
    }

    pub fn init(&self) -> std::io::Result<()> {
        self.with_conn(|_conn| Ok(()))
            .map_err(|e| std::io::Error::other(e.to_string()))
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

    fn now_ts() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }
}

impl Storage for SqliteStorage {
    fn load_last(&self) -> std::io::Result<Option<Block>> {
        let r = self.with_conn(|conn| {
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
        });
        match r {
            Ok(opt) => Ok(opt),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    }

    fn save_last(&self, height: u64, hash: &str) -> std::io::Result<()> {
        let r = self.with_conn(|conn| {
            let ts = Self::now_ts();
            conn.execute(
                "INSERT INTO chain_state (id, height, hash, updated_at) VALUES (1, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET height=excluded.height, hash=excluded.hash, updated_at=excluded.updated_at",
                params![height as i64, hash, ts],
            )?;
            Ok(())
        });
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(std::io::Error::other(e.to_string())),
        }
    }

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
    use rusqlite::{Connection, OptionalExtension};

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

        let collections = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='collections'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap();
        assert_eq!(collections.as_deref(), Some("collections"));
    }
}
