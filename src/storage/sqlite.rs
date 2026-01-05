use anyhow::Result;
use bitcoin::{hashes::Hash as _, OutPoint};
use ethereum_types::H160;
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use std::{path::Path, str::FromStr};

use super::{
    traits::{
        Collection, CollectionKey, OwnershipRange, Storage, StorageRead, StorageTx, StorageWrite,
    },
    Block,
};

const DB_SCHEMA_VERSION: i64 = 2;

const U96_BLOB_LEN: usize = 12;
const U96_MAX: u128 = (1u128 << 96) - 1;

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

fn encode_u96_be(value: u128) -> Result<[u8; U96_BLOB_LEN]> {
    if value > U96_MAX {
        anyhow::bail!("value {value} exceeds u96 max {U96_MAX}");
    }
    let bytes = value.to_be_bytes();
    let mut out = [0u8; U96_BLOB_LEN];
    out.copy_from_slice(&bytes[bytes.len() - U96_BLOB_LEN..]);
    Ok(out)
}

fn decode_u96_be(blob: &[u8]) -> Result<u128> {
    if blob.len() != U96_BLOB_LEN {
        anyhow::bail!("expected u96 blob of len {U96_BLOB_LEN}, got {}", blob.len());
    }
    let mut bytes = [0u8; 16];
    let start = bytes.len() - U96_BLOB_LEN;
    bytes[start..].copy_from_slice(blob);
    Ok(u128::from_be_bytes(bytes))
}

fn db_has_unspent_slot_overlap(
    conn: &Connection,
    collection_id: &CollectionKey,
    slot_start: u128,
    slot_end: u128,
) -> rusqlite::Result<bool> {
    let start_blob = encode_u96_be(slot_start).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Blob,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())),
        )
    })?;
    let end_blob = encode_u96_be(slot_end).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Blob,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())),
        )
    })?;

    let existing: Option<i64> = conn
        .query_row(
            r#"
            SELECT 1
            FROM ownership_ranges
            WHERE collection_id = ?1
              AND spent_height IS NULL
              AND slot_start <= ?2
              AND slot_end >= ?3
            LIMIT 1
            "#,
            params![
                collection_id.to_string(),
                end_blob.as_slice(),
                start_blob.as_slice()
            ],
            |row| row.get(0),
        )
        .optional()?;

    Ok(existing.is_some())
}

fn map_ownership_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OwnershipRange> {
    let collection_id_str: String = row.get(0)?;
    let collection_id = CollectionKey::from_str(&collection_id_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))?;

    let owner_bytes: Vec<u8> = row.get(1)?;
    if owner_bytes.len() != 20 {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            1,
            Type::Blob,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "invalid owner_h160 length: expected 20 got {}",
                    owner_bytes.len()
                ),
            )),
        ));
    }
    let owner_h160 = H160::from_slice(&owner_bytes);

    let slot_start_blob: Vec<u8> = row.get(2)?;
    let slot_end_blob: Vec<u8> = row.get(3)?;
    let slot_start = decode_u96_be(&slot_start_blob).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            Type::Blob,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())),
        )
    })?;
    let slot_end = decode_u96_be(&slot_end_blob).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            Type::Blob,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())),
        )
    })?;

    let out_txid_blob: Vec<u8> = row.get(4)?;
    let out_txid = bitcoin::Txid::from_slice(&out_txid_blob).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(4, Type::Blob, Box::new(err))
    })?;

    let out_vout_int: i64 = row.get(5)?;
    let out_vout: u32 = out_vout_int.try_into().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(5, Type::Integer, Box::new(err))
    })?;

    let created_height_int: i64 = row.get(6)?;
    let created_height: u64 = created_height_int.try_into().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(6, Type::Integer, Box::new(err))
    })?;

    let created_tx_index_int: i64 = row.get(7)?;
    let created_tx_index: u32 = created_tx_index_int.try_into().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(7, Type::Integer, Box::new(err))
    })?;

    Ok(OwnershipRange {
        owner_h160,
        collection_id,
        outpoint: OutPoint {
            txid: out_txid,
            vout: out_vout,
        },
        slot_start,
        slot_end,
        created_height,
        created_tx_index,
    })
}

fn db_list_unspent_ownership_by_owner(
    conn: &Connection,
    owner_h160: H160,
) -> rusqlite::Result<Vec<OwnershipRange>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT collection_id, owner_h160, slot_start, slot_end, out_txid, out_vout, created_height, created_tx_index
        FROM ownership_ranges
        WHERE owner_h160 = ?1
          AND spent_height IS NULL
        ORDER BY created_height, created_tx_index, out_txid, out_vout, slot_start
        "#,
    )?;
    let rows = stmt
        .query_map(params![owner_h160.as_bytes()], map_ownership_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn db_list_unspent_ownership_by_owners(
    conn: &Connection,
    owners: &[H160],
) -> rusqlite::Result<Vec<OwnershipRange>> {
    if owners.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for chunk in owners.chunks(900) {
        let placeholders = std::iter::repeat("?")
            .take(chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT collection_id, owner_h160, slot_start, slot_end, out_txid, out_vout, created_height, created_tx_index
            FROM ownership_ranges
            WHERE spent_height IS NULL
              AND owner_h160 IN ({placeholders})
            ORDER BY owner_h160, created_height, created_tx_index, out_txid, out_vout, slot_start
            "#
        );

        let params_vec = chunk
            .iter()
            .map(|h| h.as_bytes().to_vec())
            .collect::<Vec<_>>();

        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt
            .query_map(rusqlite::params_from_iter(params_vec.iter()), map_ownership_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        out.extend(mapped);
    }

    Ok(out)
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

fn db_insert_ownership_range(
    conn: &Connection,
    collection_id: CollectionKey,
    owner_h160: H160,
    outpoint: OutPoint,
    slot_start: u128,
    slot_end: u128,
    created_height: u64,
    created_tx_index: u32,
) -> rusqlite::Result<()> {
    let start_blob = encode_u96_be(slot_start).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Blob,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())),
        )
    })?;
    let end_blob = encode_u96_be(slot_end).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Blob,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())),
        )
    })?;
    let out_txid_bytes = outpoint.txid.to_byte_array();

    conn.execute(
        r#"
        INSERT INTO ownership_ranges (
            collection_id, owner_h160, slot_start, slot_end, out_txid, out_vout,
            created_height, created_tx_index, spent_height, spent_txid
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL)
        "#,
        params![
            collection_id.to_string(),
            owner_h160.as_bytes(),
            start_blob.as_slice(),
            end_blob.as_slice(),
            &out_txid_bytes[..],
            outpoint.vout as i64,
            created_height as i64,
            created_tx_index as i64
        ],
    )?;
    Ok(())
}

fn db_mark_ownership_outpoint_spent(
    conn: &Connection,
    outpoint: OutPoint,
    spent_height: u64,
    spent_txid: bitcoin::Txid,
) -> rusqlite::Result<usize> {
    let spent_txid_bytes = spent_txid.to_byte_array();
    let out_txid_bytes = outpoint.txid.to_byte_array();
    let rows = conn.execute(
        r#"
        UPDATE ownership_ranges
        SET spent_height = ?1,
            spent_txid = ?2
        WHERE out_txid = ?3
          AND out_vout = ?4
          AND spent_height IS NULL
        "#,
        params![
            spent_height as i64,
            &spent_txid_bytes[..],
            &out_txid_bytes[..],
            outpoint.vout as i64
        ],
    )?;
    Ok(rows)
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

    fn has_unspent_slot_overlap(
        &self,
        collection_id: &CollectionKey,
        slot_start: u128,
        slot_end: u128,
    ) -> Result<bool> {
        Ok(db_has_unspent_slot_overlap(
            &self.conn,
            collection_id,
            slot_start,
            slot_end,
        )?)
    }

    fn list_unspent_ownership_by_owner(&self, owner_h160: H160) -> Result<Vec<OwnershipRange>> {
        Ok(db_list_unspent_ownership_by_owner(&self.conn, owner_h160)?)
    }

    fn list_unspent_ownership_by_owners(
        &self,
        owner_h160s: &[H160],
    ) -> Result<Vec<OwnershipRange>> {
        Ok(db_list_unspent_ownership_by_owners(&self.conn, owner_h160s)?)
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

    fn insert_ownership_range(
        &self,
        collection_id: CollectionKey,
        owner_h160: H160,
        outpoint: OutPoint,
        slot_start: u128,
        slot_end: u128,
        created_height: u64,
        created_tx_index: u32,
    ) -> Result<()> {
        Ok(db_insert_ownership_range(
            &self.conn,
            collection_id,
            owner_h160,
            outpoint,
            slot_start,
            slot_end,
            created_height,
            created_tx_index,
        )?)
    }

    fn mark_ownership_outpoint_spent(
        &self,
        outpoint: OutPoint,
        spent_height: u64,
        spent_txid: bitcoin::Txid,
    ) -> Result<usize> {
        Ok(db_mark_ownership_outpoint_spent(
            &self.conn,
            outpoint,
            spent_height,
            spent_txid,
        )?)
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

        log::info!(
            "SQLite schema migration: {} -> {}",
            version,
            DB_SCHEMA_VERSION
        );

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
            CREATE TABLE ownership_ranges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                collection_id TEXT NOT NULL,
                owner_h160 BLOB NOT NULL CHECK (length(owner_h160) = 20),
                slot_start BLOB NOT NULL CHECK (length(slot_start) = 12),
                slot_end BLOB NOT NULL CHECK (length(slot_end) = 12),
                out_txid BLOB NOT NULL CHECK (length(out_txid) = 32),
                out_vout INTEGER NOT NULL,
                created_height INTEGER NOT NULL,
                created_tx_index INTEGER NOT NULL,
                spent_height INTEGER,
                spent_txid BLOB CHECK (spent_txid IS NULL OR length(spent_txid) = 32)
            );
            CREATE INDEX ownership_ranges_owner_unspent_idx
                ON ownership_ranges(owner_h160)
                WHERE spent_height IS NULL;
            CREATE INDEX ownership_ranges_outpoint_unspent_idx
                ON ownership_ranges(out_txid, out_vout)
                WHERE spent_height IS NULL;
            CREATE INDEX ownership_ranges_collection_unspent_idx
                ON ownership_ranges(collection_id, slot_start, slot_end)
                WHERE spent_height IS NULL;
        "#,
            )?;
            conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            return Ok(());
        }

        if version == 1 {
            conn.execute_batch(
                r#"
            CREATE TABLE ownership_ranges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                collection_id TEXT NOT NULL,
                owner_h160 BLOB NOT NULL CHECK (length(owner_h160) = 20),
                slot_start BLOB NOT NULL CHECK (length(slot_start) = 12),
                slot_end BLOB NOT NULL CHECK (length(slot_end) = 12),
                out_txid BLOB NOT NULL CHECK (length(out_txid) = 32),
                out_vout INTEGER NOT NULL,
                created_height INTEGER NOT NULL,
                created_tx_index INTEGER NOT NULL,
                spent_height INTEGER,
                spent_txid BLOB CHECK (spent_txid IS NULL OR length(spent_txid) = 32)
            );
            CREATE INDEX ownership_ranges_owner_unspent_idx
                ON ownership_ranges(owner_h160)
                WHERE spent_height IS NULL;
            CREATE INDEX ownership_ranges_outpoint_unspent_idx
                ON ownership_ranges(out_txid, out_vout)
                WHERE spent_height IS NULL;
            CREATE INDEX ownership_ranges_collection_unspent_idx
                ON ownership_ranges(collection_id, slot_start, slot_end)
                WHERE spent_height IS NULL;
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

    fn has_unspent_slot_overlap(
        &self,
        collection_id: &CollectionKey,
        slot_start: u128,
        slot_end: u128,
    ) -> Result<bool> {
        let overlap =
            self.with_conn(|conn| db_has_unspent_slot_overlap(conn, collection_id, slot_start, slot_end))?;
        Ok(overlap)
    }

    fn list_unspent_ownership_by_owner(&self, owner_h160: H160) -> Result<Vec<OwnershipRange>> {
        let rows = self.with_conn(|conn| db_list_unspent_ownership_by_owner(conn, owner_h160))?;
        Ok(rows)
    }

    fn list_unspent_ownership_by_owners(
        &self,
        owner_h160s: &[H160],
    ) -> Result<Vec<OwnershipRange>> {
        let rows =
            self.with_conn(|conn| db_list_unspent_ownership_by_owners(conn, owner_h160s))?;
        Ok(rows)
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
