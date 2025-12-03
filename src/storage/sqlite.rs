use anyhow::Result;
use bitcoin::{OutPoint, Txid};
use ethereum_types::H160;
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use std::{io, path::Path, str::FromStr};

use super::{
    token::{TokenKey, TokenOwnership},
    traits::{Collection, CollectionKey, Storage, StorageRead, StorageTx, StorageWrite},
    Block,
};
use crate::types::{SlotNumber, TokenId};

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

fn blob_length_error(column: usize, expected: usize, actual: usize) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        Type::Blob,
        Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {expected} bytes, got {actual}"),
        )),
    )
}

fn blob_to_array<const N: usize>(blob: Vec<u8>, column: usize) -> rusqlite::Result<[u8; N]> {
    if blob.len() != N {
        return Err(blob_length_error(column, N, blob.len()));
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&blob);
    Ok(out)
}

fn db_load_token(conn: &Connection, key: &TokenKey) -> rusqlite::Result<Option<TokenOwnership>> {
    let mut stmt = conn.prepare(
        "SELECT collection_id, slot, initial_owner_h160, owner_txid, owner_vout, block_height, tx_index
         FROM token_ownerships
         WHERE collection_id = ?1 AND slot = ?2 AND initial_owner_h160 = ?3",
    )?;
    stmt.query_row(
        (
            key.collection.to_string(),
            key.token_id.slot().to_be_bytes().to_vec(),
            key.token_id.initial_owner().to_vec(),
        ),
        map_token_row,
    )
    .optional()
}

fn db_save_token(conn: &Connection, token: &TokenOwnership) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO token_ownerships (
            collection_id,
            slot,
            initial_owner_h160,
            owner_txid,
            owner_vout,
            block_height,
            tx_index
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            token.key.collection.to_string(),
            token.key.token_id.slot().to_be_bytes().to_vec(),
            token.key.token_id.initial_owner().to_vec(),
            token.owner_outpoint.txid.to_string(),
            token.owner_outpoint.vout as i64,
            token.registered_block_height as i64,
            token.registered_tx_index as i64
        ],
    )?;
    Ok(())
}

fn map_token_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TokenOwnership> {
    let collection_id: String = row.get(0)?;
    let slot_blob: Vec<u8> = row.get(1)?;
    let owner_blob: Vec<u8> = row.get(2)?;
    let owner_txid_hex: String = row.get(3)?;
    let owner_vout: i64 = row.get(4)?;
    let block_height: i64 = row.get(5)?;
    let tx_index: i64 = row.get(6)?;

    let collection = CollectionKey::from_str(&collection_id)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))?;

    if owner_vout < 0 {
        return Err(rusqlite::Error::IntegralValueOutOfRange(4, owner_vout));
    }
    if block_height < 0 {
        return Err(rusqlite::Error::IntegralValueOutOfRange(5, block_height));
    }
    if tx_index < 0 {
        return Err(rusqlite::Error::IntegralValueOutOfRange(6, tx_index));
    }

    let slot_bytes = blob_to_array::<12>(slot_blob, 1)?;
    let slot = SlotNumber::from_be_bytes(&slot_bytes)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(1, Type::Blob, Box::new(err)))?;
    let owner_bytes = blob_to_array::<20>(owner_blob, 2)?;
    let token_id = TokenId::new(slot, owner_bytes);

    let txid = Txid::from_str(&owner_txid_hex)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(3, Type::Text, Box::new(err)))?;
    let outpoint = OutPoint {
        txid,
        vout: owner_vout as u32,
    };

    Ok(TokenOwnership {
        key: TokenKey::new(collection, token_id),
        owner_outpoint: outpoint,
        registered_block_height: block_height as u64,
        registered_tx_index: tx_index as u32,
    })
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

    fn load_token(&self, key: &TokenKey) -> Result<Option<TokenOwnership>> {
        Ok(db_load_token(&self.conn, key)?)
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

    fn save_token(&self, token: &TokenOwnership) -> Result<()> {
        Ok(db_save_token(&self.conn, token)?)
    }
}

impl Storage for SqliteStorage {
    type Tx = SqliteTx;

    fn begin_tx(&self) -> Result<Self::Tx> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_millis(500))?;
        Self::migrate(&conn)?;

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
            CREATE TABLE token_ownerships (
                collection_id TEXT NOT NULL,
                slot BLOB NOT NULL CHECK(length(slot) = 12),
                initial_owner_h160 BLOB NOT NULL CHECK(length(initial_owner_h160) = 20),
                owner_txid TEXT NOT NULL,
                owner_vout INTEGER NOT NULL,
                block_height INTEGER NOT NULL,
                tx_index INTEGER NOT NULL,
                PRIMARY KEY (collection_id, slot, initial_owner_h160)
            );
            CREATE INDEX token_owner_outpoint_idx ON token_ownerships(owner_txid, owner_vout);
        "#,
            )?;
            conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            return Ok(());
        }

        if version == 1 {
            conn.execute_batch(
                r#"
            CREATE TABLE token_ownerships (
                collection_id TEXT NOT NULL,
                slot BLOB NOT NULL CHECK(length(slot) = 12),
                initial_owner_h160 BLOB NOT NULL CHECK(length(initial_owner_h160) = 20),
                owner_txid TEXT NOT NULL,
                owner_vout INTEGER NOT NULL,
                block_height INTEGER NOT NULL,
                tx_index INTEGER NOT NULL,
                PRIMARY KEY (collection_id, slot, initial_owner_h160)
            );
            CREATE INDEX token_owner_outpoint_idx ON token_ownerships(owner_txid, owner_vout);
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

    fn load_token(&self, key: &TokenKey) -> Result<Option<TokenOwnership>> {
        let row = self.with_conn(|conn| db_load_token(conn, key))?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::DB_SCHEMA_VERSION;
    use crate::storage::traits::{TokenKey, TokenOwnership};
    use crate::types::{SlotNumber, TokenId};
    use bitcoin::Txid;
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

    #[test]
    fn sqlite_migrates_from_version_one() {
        let path = unique_temp_file("brc721_migrate_v1", "db");
        {
            let conn = Connection::open(&path).unwrap();
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
            PRAGMA user_version = 1;
        "#,
            )
            .unwrap();
        }

        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        let conn = Connection::open(&path).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, DB_SCHEMA_VERSION);
        let has_table = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='token_ownerships'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap();
        assert_eq!(has_table.as_deref(), Some("token_ownerships"));
    }

    fn sample_token(collection: CollectionKey) -> TokenOwnership {
        let slot = SlotNumber::new(5).unwrap();
        let mut owner = [0u8; 20];
        owner[19] = 0xAA;
        let token_id = TokenId::new(slot, owner);
        TokenOwnership {
            key: TokenKey::new(collection, token_id),
            owner_outpoint: OutPoint {
                txid: Txid::from_str(
                    "5a02c7bb0a55dfc8f915cb490df29262552d2b2c69f0a7f2bd908d1c8d3f9abc",
                )
                .unwrap(),
                vout: 1,
            },
            registered_block_height: 100,
            registered_tx_index: 2,
        }
    }

    #[test]
    fn sqlite_save_and_load_token() {
        let path = unique_temp_file("brc721_save_token", "db");
        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        let collection = CollectionKey::new(10, 0);
        let token = sample_token(collection.clone());

        let tx = repo.begin_tx().unwrap();
        tx.save_token(&token).unwrap();
        let loaded = tx
            .load_token(&token.key)
            .unwrap()
            .expect("token should exist");
        assert_eq!(loaded, token);
        tx.commit().unwrap();

        let fetched = repo
            .load_token(&token.key)
            .unwrap()
            .expect("token via storage");
        assert_eq!(fetched, token);
    }
}
