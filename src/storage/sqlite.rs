use anyhow::Result;
use ethereum_types::H160;
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use std::{path::Path, str::FromStr};

use super::{
    traits::{
        Collection, CollectionKey, RegisteredToken, RegisteredTokenSave, Storage, StorageRead,
        StorageTx, StorageWrite,
    },
    Block,
};

const DB_SCHEMA_VERSION: i64 = 2;

#[derive(Clone)]
pub struct SqliteStorage {
    pub path: String,
}

pub struct SqliteTx {
    conn: Connection,
}

impl StorageTx for SqliteTx {
    fn commit(self) -> Result<()> {
        self.conn.execute("COMMIT", [])?;
        Ok(())
    }
}

fn db_load_last(conn: &Connection) -> rusqlite::Result<Option<Block>> {
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
}

fn map_collection_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Collection> {
    let id: String = row.get(0)?;
    let key = CollectionKey::from_str(&id)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))?;
    let evm_collection_address_str: String = row.get(1)?;
    let evm_collection_address = H160::from_str(&evm_collection_address_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(err)))?;
    let rebaseable_int: i64 = row.get(2)?;
    let rebaseable = rebaseable_int != 0;
    Ok(Collection {
        key,
        evm_collection_address,
        rebaseable,
    })
}

fn db_load_collection(
    conn: &Connection,
    key: &CollectionKey,
) -> rusqlite::Result<Option<Collection>> {
    conn.query_row(
        "SELECT id, evm_collection_address, rebaseable FROM collections WHERE id = ?1",
        params![key.to_string()],
        map_collection_row,
    )
    .optional()
}

fn db_list_collections(conn: &Connection) -> rusqlite::Result<Vec<Collection>> {
    let mut stmt =
        conn.prepare("SELECT id, evm_collection_address, rebaseable FROM collections ORDER BY id")?;
    let mapped = stmt
        .query_map([], map_collection_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mapped)
}

fn db_save_last(conn: &Connection, height: u64, hash: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO chain_state (id, height, hash) VALUES (1, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET height=excluded.height, hash=excluded.hash",
        params![height as i64, hash],
    )?;
    Ok(())
}

fn db_save_collection(
    conn: &Connection,
    key: CollectionKey,
    evm_collection_address: H160,
    rebaseable: bool,
) -> rusqlite::Result<()> {
    let id = key.to_string();
    conn.execute(
        "INSERT INTO collections (id, evm_collection_address, rebaseable) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET evm_collection_address=excluded.evm_collection_address, rebaseable=excluded.rebaseable",
        params![id, format!("0x{:x}", evm_collection_address), rebaseable as i64],
    )?;
    Ok(())
}

fn map_registered_token_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RegisteredToken> {
    let collection_id_str: String = row.get(0)?;
    let collection_id = CollectionKey::from_str(&collection_id_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))?;

    let token_id: String = row.get(1)?;

    let owner_h160_str: String = row.get(2)?;
    let owner_h160 = H160::from_str(&owner_h160_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(2, Type::Text, Box::new(err)))?;

    let reg_txid: String = row.get(3)?;

    let reg_vout_raw: i64 = row.get(4)?;
    let reg_vout: u32 = reg_vout_raw
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(4, reg_vout_raw))?;

    let created_height_raw: i64 = row.get(5)?;
    let created_height: u64 = created_height_raw
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(5, created_height_raw))?;

    let created_tx_index_raw: i64 = row.get(6)?;
    let created_tx_index: u32 = created_tx_index_raw
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(6, created_tx_index_raw))?;

    Ok(RegisteredToken {
        collection_id,
        token_id,
        owner_h160,
        reg_txid,
        reg_vout,
        created_height,
        created_tx_index,
    })
}

fn db_load_registered_token(
    conn: &Connection,
    collection_id: &CollectionKey,
    token_id: &str,
) -> rusqlite::Result<Option<RegisteredToken>> {
    conn.query_row(
        r#"
        SELECT collection_id, token_id, owner_h160, reg_txid, reg_vout, created_height, created_tx_index
        FROM registered_tokens
        WHERE collection_id = ?1 AND token_id = ?2
        "#,
        params![collection_id.to_string(), token_id],
        map_registered_token_row,
    )
    .optional()
}

fn db_save_registered_token(
    conn: &Connection,
    token: RegisteredTokenSave<'_>,
) -> rusqlite::Result<()> {
    conn.execute(
        r#"
        INSERT INTO registered_tokens (
            collection_id, token_id, owner_h160, reg_txid, reg_vout, created_height, created_tx_index
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(collection_id, token_id) DO NOTHING
        "#,
        params![
            token.collection_id.to_string(),
            token.token_id,
            format!("0x{:x}", token.owner_h160),
            token.reg_txid,
            token.reg_vout as i64,
            token.created_height as i64,
            token.created_tx_index as i64,
        ],
    )?;
    Ok(())
}

impl StorageRead for SqliteTx {
    fn load_last(&self) -> Result<Option<Block>> {
        Ok(db_load_last(&self.conn)?)
    }

    fn load_collection(&self, id: &CollectionKey) -> Result<Option<Collection>> {
        Ok(db_load_collection(&self.conn, id)?)
    }

    fn list_collections(&self) -> Result<Vec<Collection>> {
        Ok(db_list_collections(&self.conn)?)
    }

    fn load_registered_token(
        &self,
        collection_id: &CollectionKey,
        token_id: &str,
    ) -> Result<Option<RegisteredToken>> {
        Ok(db_load_registered_token(
            &self.conn,
            collection_id,
            token_id,
        )?)
    }
}

impl StorageWrite for SqliteTx {
    fn save_last(&self, height: u64, hash: &str) -> Result<()> {
        Ok(db_save_last(&self.conn, height, hash)?)
    }

    fn save_collection(
        &self,
        key: CollectionKey,
        evm_collection_address: H160,
        rebaseable: bool,
    ) -> Result<()> {
        Ok(db_save_collection(
            &self.conn,
            key,
            evm_collection_address,
            rebaseable,
        )?)
    }

    fn save_registered_token(&self, token: RegisteredTokenSave<'_>) -> Result<()> {
        Ok(db_save_registered_token(&self.conn, token)?)
    }
}

impl Storage for SqliteStorage {
    type Tx = SqliteTx;

    fn begin_tx(&self) -> Result<Self::Tx> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_millis(500))?;

        conn.execute("BEGIN IMMEDIATE", [])?;

        Ok(SqliteTx { conn })
    }
}

impl SqliteStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_string_lossy().to_string(),
        }
    }

    pub fn reset_all(&self) -> Result<()> {
        if !std::path::Path::new(&self.path).exists() {
            return Ok(());
        }
        std::fs::remove_file(&self.path)?;
        Ok(())
    }

    pub fn init(&self) -> Result<()> {
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
            CREATE TABLE registered_tokens (
                collection_id TEXT NOT NULL,
                token_id TEXT NOT NULL,
                owner_h160 TEXT NOT NULL,
                reg_txid TEXT NOT NULL,
                reg_vout INTEGER NOT NULL,
                created_height INTEGER NOT NULL,
                created_tx_index INTEGER NOT NULL,
                PRIMARY KEY (collection_id, token_id)
            );
            CREATE INDEX registered_tokens_owner_idx ON registered_tokens(owner_h160);
        "#,
            )?;
            conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            log::info!("🗄️ Initialized SQLite schema to v{}", DB_SCHEMA_VERSION);
            return Ok(());
        }

        if version == 1 {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS registered_tokens (
                    collection_id TEXT NOT NULL,
                    token_id TEXT NOT NULL,
                    owner_h160 TEXT NOT NULL,
                    reg_txid TEXT NOT NULL,
                    reg_vout INTEGER NOT NULL,
                    created_height INTEGER NOT NULL,
                    created_tx_index INTEGER NOT NULL,
                    PRIMARY KEY (collection_id, token_id)
                );
                CREATE INDEX IF NOT EXISTS registered_tokens_owner_idx ON registered_tokens(owner_h160);
                "#,
            )?;
            conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            log::info!(
                "🗄️ Migrated SQLite schema from v{} to v{}",
                version,
                DB_SCHEMA_VERSION
            );
            return Ok(());
        }

        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::SchemaChanged as i32),
            Some("database schema version mismatch; please run with --reset option".to_string()),
        ))
    }
}

impl StorageRead for SqliteStorage {
    fn load_last(&self) -> Result<Option<Block>> {
        let opt = self.with_conn(db_load_last)?;
        Ok(opt)
    }

    fn load_collection(&self, id: &CollectionKey) -> Result<Option<Collection>> {
        let row = self.with_conn(|conn| db_load_collection(conn, id))?;
        Ok(row)
    }

    fn list_collections(&self) -> Result<Vec<Collection>> {
        let rows = self.with_conn(db_list_collections)?;
        Ok(rows)
    }

    fn load_registered_token(
        &self,
        collection_id: &CollectionKey,
        token_id: &str,
    ) -> Result<Option<RegisteredToken>> {
        let row = self.with_conn(|conn| db_load_registered_token(conn, collection_id, token_id))?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::DB_SCHEMA_VERSION;
    use rusqlite::{Connection, OptionalExtension};
    use std::{
        str::FromStr,
        time::{SystemTime, UNIX_EPOCH},
    };

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

        let repo = repo.begin_tx().unwrap();
        assert_eq!(repo.load_last().unwrap(), None);

        repo.save_last(100, "hash100").unwrap();
        let first = repo.load_last().unwrap().unwrap();
        assert_eq!(first.height, 100);
        assert_eq!(first.hash, "hash100");

        repo.save_last(101, "hash101").unwrap();
        let second = repo.load_last().unwrap().unwrap();
        assert_eq!(second.height, 101);
        assert_eq!(second.hash, "hash101");
        repo.commit().unwrap();

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

        let repo = repo.begin_tx().unwrap();
        repo.save_collection(
            CollectionKey::new(123, 0),
            H160::from_str("0xaaaa000000000000000000000000000000000000").unwrap(),
            true,
        )
        .unwrap();
        repo.save_collection(
            CollectionKey::new(124, 1),
            H160::from_str("0xbbbb000000000000000000000000000000000000").unwrap(),
            false,
        )
        .unwrap();

        let loaded = repo
            .load_collection(&CollectionKey::new(123, 0))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.key.to_string(), "123:0");
        assert_eq!(
            loaded.evm_collection_address,
            H160::from_str("0xaaaa000000000000000000000000000000000000").unwrap()
        );
        assert!(loaded.rebaseable);
        assert!(repo
            .load_collection(&CollectionKey::new(999, 9))
            .unwrap()
            .is_none());

        let collections = repo.list_collections().unwrap();
        assert_eq!(collections.len(), 2);
        assert_eq!(collections[0].key.to_string(), "123:0");
        assert_eq!(
            collections[0].evm_collection_address,
            H160::from_str("0xaaaa000000000000000000000000000000000000").unwrap()
        );
        assert!(collections[0].rebaseable);
        assert_eq!(collections[1].key.to_string(), "124:1");
        assert_eq!(
            collections[1].evm_collection_address,
            H160::from_str("0xbbbb000000000000000000000000000000000000").unwrap()
        );
        assert!(!collections[1].rebaseable);
    }

    #[test]
    fn sqlite_allows_duplicate_evm_collection_addresses() {
        let path = unique_temp_file("brc721_dup_addr", "db");
        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        let tx = repo.begin_tx().unwrap();
        let duplicate_addr = H160::from_str("0xcccc000000000000000000000000000000000000").unwrap();
        tx.save_collection(CollectionKey::new(200, 0), duplicate_addr, true)
            .unwrap();
        tx.save_collection(CollectionKey::new(201, 1), duplicate_addr, false)
            .unwrap();
        tx.commit().unwrap();

        let collections = repo.list_collections().unwrap();
        assert_eq!(collections.len(), 2);
        assert_eq!(collections[0].key.to_string(), "200:0");
        assert_eq!(collections[1].key.to_string(), "201:1");
        assert_eq!(collections[0].evm_collection_address, duplicate_addr);
        assert_eq!(collections[1].evm_collection_address, duplicate_addr);
    }

    #[test]
    fn sqlite_transaction_commit_persists_data() {
        let path = unique_temp_file("brc721_tx_commit", "db");
        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        let tx = repo.begin_tx().unwrap();
        tx.save_last(200, "hash200").unwrap();
        tx.commit().unwrap();

        let last = repo.load_last().unwrap().unwrap();
        assert_eq!(last.height, 200);
        assert_eq!(last.hash, "hash200");
    }
}
