use anyhow::Result;
use ethereum_types::H160;
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use std::{path::Path, str::FromStr};

use super::{
    traits::{
        Collection, CollectionKey, OwnershipRange, OwnershipRangeWithGroup, OwnershipUtxo,
        OwnershipUtxoSave, Storage, StorageRead, StorageTx, StorageWrite,
    },
    Block,
};

const DB_SCHEMA_VERSION: i64 = 7;

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

const SLOT96_BLOB_LEN: usize = 12;

fn encode_slot96(slot: u128) -> [u8; SLOT96_BLOB_LEN] {
    let bytes = slot.to_be_bytes();
    let mut out = [0u8; SLOT96_BLOB_LEN];
    out.copy_from_slice(&bytes[bytes.len() - SLOT96_BLOB_LEN..]);
    out
}

fn decode_slot96(bytes: &[u8], col: usize) -> rusqlite::Result<u128> {
    if bytes.len() != SLOT96_BLOB_LEN {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            col,
            Type::Blob,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "invalid slot blob length {}, expected {}",
                    bytes.len(),
                    SLOT96_BLOB_LEN
                ),
            )),
        ));
    }

    let mut buf = [0u8; 16];
    let start = buf.len() - SLOT96_BLOB_LEN;
    buf[start..].copy_from_slice(bytes);
    Ok(u128::from_be_bytes(buf))
}

fn map_ownership_utxo_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OwnershipUtxo> {
    let collection_id_str: String = row.get(0)?;
    let collection_id = CollectionKey::from_str(&collection_id_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))?;

    let reg_txid: String = row.get(1)?;

    let reg_vout_raw: i64 = row.get(2)?;
    let reg_vout: u32 = reg_vout_raw
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, reg_vout_raw))?;

    let owner_h160_str: String = row.get(3)?;
    let owner_h160 = H160::from_str(&owner_h160_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(3, Type::Text, Box::new(err)))?;

    let owner_script_pubkey: Vec<u8> = row.get(4)?;

    let base_h160_str: String = row.get(5)?;
    let base_h160 = H160::from_str(&base_h160_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(5, Type::Text, Box::new(err)))?;

    let created_height_raw: i64 = row.get(6)?;
    let created_height: u64 = created_height_raw
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(6, created_height_raw))?;

    let created_tx_index_raw: i64 = row.get(7)?;
    let created_tx_index: u32 = created_tx_index_raw
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(7, created_tx_index_raw))?;

    let spent_txid: Option<String> = row.get(8)?;

    let spent_height_raw: Option<i64> = row.get(9)?;
    let spent_height = match spent_height_raw {
        Some(raw) => Some(
            raw.try_into()
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(9, raw))?,
        ),
        None => None,
    };

    let spent_tx_index_raw: Option<i64> = row.get(10)?;
    let spent_tx_index = match spent_tx_index_raw {
        Some(raw) => Some(
            raw.try_into()
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(10, raw))?,
        ),
        None => None,
    };

    Ok(OwnershipUtxo {
        collection_id,
        reg_txid,
        reg_vout,
        owner_h160,
        owner_script_pubkey,
        base_h160,
        created_height,
        created_tx_index,
        spent_txid,
        spent_height,
        spent_tx_index,
    })
}

fn map_ownership_range_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OwnershipRange> {
    let slot_start_blob: Vec<u8> = row.get(0)?;
    let slot_end_blob: Vec<u8> = row.get(1)?;
    let slot_start = decode_slot96(&slot_start_blob, 0)?;
    let slot_end = decode_slot96(&slot_end_blob, 1)?;
    Ok(OwnershipRange {
        slot_start,
        slot_end,
    })
}

fn map_ownership_range_with_group_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<OwnershipRangeWithGroup> {
    let collection_id_str: String = row.get(0)?;
    let collection_id = CollectionKey::from_str(&collection_id_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(err)))?;

    let base_h160_str: String = row.get(1)?;
    let base_h160 = H160::from_str(&base_h160_str)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(1, Type::Text, Box::new(err)))?;

    let slot_start_blob: Vec<u8> = row.get(2)?;
    let slot_end_blob: Vec<u8> = row.get(3)?;
    let slot_start = decode_slot96(&slot_start_blob, 2)?;
    let slot_end = decode_slot96(&slot_end_blob, 3)?;

    Ok(OwnershipRangeWithGroup {
        collection_id,
        base_h160,
        slot_start,
        slot_end,
    })
}

fn db_list_unspent_ownership_utxos_by_outpoint(
    conn: &Connection,
    reg_txid: &str,
    reg_vout: u32,
) -> rusqlite::Result<Vec<OwnershipUtxo>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            collection_id, reg_txid, reg_vout, owner_h160, owner_script_pubkey, base_h160,
            created_height, created_tx_index,
            spent_txid, spent_height, spent_tx_index
        FROM ownership_utxos
        WHERE reg_txid = ?1 AND reg_vout = ?2 AND spent_txid IS NULL
        ORDER BY collection_id, base_h160
        "#,
    )?;
    let mapped = stmt
        .query_map(params![reg_txid, reg_vout as i64], map_ownership_utxo_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mapped)
}

fn db_list_unspent_ownership_ranges_by_outpoint(
    conn: &Connection,
    reg_txid: &str,
    reg_vout: u32,
) -> rusqlite::Result<Vec<OwnershipRangeWithGroup>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT r.collection_id, r.base_h160, r.slot_start, r.slot_end
        FROM ownership_ranges r
        JOIN ownership_utxos u
            ON r.reg_txid = u.reg_txid
            AND r.reg_vout = u.reg_vout
            AND r.collection_id = u.collection_id
            AND r.base_h160 = u.base_h160
        WHERE r.reg_txid = ?1 AND r.reg_vout = ?2 AND u.spent_txid IS NULL
        ORDER BY r.range_seq
        "#,
    )?;
    let mapped = stmt
        .query_map(
            params![reg_txid, reg_vout as i64],
            map_ownership_range_with_group_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mapped)
}

fn db_list_ownership_ranges(
    conn: &Connection,
    utxo: &OwnershipUtxo,
) -> rusqlite::Result<Vec<OwnershipRange>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT slot_start, slot_end
        FROM ownership_ranges
        WHERE
            reg_txid = ?1
            AND reg_vout = ?2
            AND collection_id = ?3
            AND base_h160 = ?4
        ORDER BY range_seq
        "#,
    )?;
    let mapped = stmt
        .query_map(
            params![
                utxo.reg_txid,
                utxo.reg_vout as i64,
                utxo.collection_id.to_string(),
                format!("0x{:x}", utxo.base_h160)
            ],
            map_ownership_range_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mapped)
}

fn db_find_unspent_ownership_utxo_for_slot(
    conn: &Connection,
    collection_id: &CollectionKey,
    base_h160: H160,
    slot: u128,
) -> rusqlite::Result<Option<OwnershipUtxo>> {
    let slot_blob = encode_slot96(slot);
    conn.query_row(
        r#"
        SELECT
            u.collection_id, u.reg_txid, u.reg_vout, u.owner_h160, u.owner_script_pubkey,
            u.base_h160, u.created_height, u.created_tx_index,
            u.spent_txid, u.spent_height, u.spent_tx_index
        FROM ownership_utxos u
        JOIN ownership_ranges r
            ON r.reg_txid = u.reg_txid
            AND r.reg_vout = u.reg_vout
            AND r.collection_id = u.collection_id
            AND r.base_h160 = u.base_h160
        WHERE
            u.collection_id = ?1
            AND u.base_h160 = ?2
            AND u.spent_txid IS NULL
            AND r.slot_start <= ?3
            AND r.slot_end >= ?3
        ORDER BY u.created_height DESC, u.created_tx_index DESC
        LIMIT 1
        "#,
        params![
            collection_id.to_string(),
            format!("0x{:x}", base_h160),
            slot_blob.as_slice()
        ],
        map_ownership_utxo_row,
    )
    .optional()
}

fn db_list_unspent_ownership_utxos_by_owner(
    conn: &Connection,
    owner_h160: H160,
) -> rusqlite::Result<Vec<OwnershipUtxo>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            collection_id, reg_txid, reg_vout, owner_h160, owner_script_pubkey, base_h160,
            created_height, created_tx_index,
            spent_txid, spent_height, spent_tx_index
        FROM ownership_utxos
        WHERE owner_h160 = ?1 AND spent_txid IS NULL
        ORDER BY collection_id, reg_txid, reg_vout, base_h160
        "#,
    )?;

    let mapped = stmt
        .query_map(
            params![format!("0x{:x}", owner_h160)],
            map_ownership_utxo_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mapped)
}

fn db_save_ownership_utxo(conn: &Connection, utxo: OwnershipUtxoSave<'_>) -> rusqlite::Result<()> {
    conn.execute(
        r#"
        INSERT INTO ownership_utxos (
            collection_id,
            reg_txid,
            reg_vout,
            owner_h160,
            owner_script_pubkey,
            base_h160,
            created_height,
            created_tx_index
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(reg_txid, reg_vout, collection_id, base_h160) DO NOTHING
        "#,
        params![
            utxo.collection_id.to_string(),
            utxo.reg_txid,
            utxo.reg_vout as i64,
            format!("0x{:x}", utxo.owner_h160),
            utxo.owner_script_pubkey,
            format!("0x{:x}", utxo.base_h160),
            utxo.created_height as i64,
            utxo.created_tx_index as i64,
        ],
    )?;
    Ok(())
}

fn db_save_ownership_range(
    conn: &Connection,
    reg_txid: &str,
    reg_vout: u32,
    collection_id: &CollectionKey,
    base_h160: H160,
    slot_start: u128,
    slot_end: u128,
) -> rusqlite::Result<()> {
    let start_blob = encode_slot96(slot_start);
    let end_blob = encode_slot96(slot_end);
    conn.execute(
        r#"
        INSERT INTO ownership_ranges (
            reg_txid, reg_vout, collection_id, base_h160, slot_start, slot_end, range_seq
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            (
                SELECT COALESCE(MAX(range_seq), -1) + 1
                FROM ownership_ranges
                WHERE reg_txid = ?1 AND reg_vout = ?2
            )
        )
        ON CONFLICT(reg_txid, reg_vout, collection_id, base_h160, slot_start, slot_end) DO NOTHING
        "#,
        params![
            reg_txid,
            reg_vout as i64,
            collection_id.to_string(),
            format!("0x{:x}", base_h160),
            start_blob.as_slice(),
            end_blob.as_slice()
        ],
    )?;
    Ok(())
}

fn db_mark_ownership_utxo_spent(
    conn: &Connection,
    reg_txid: &str,
    reg_vout: u32,
    spent_txid: &str,
    spent_height: u64,
    spent_tx_index: u32,
) -> rusqlite::Result<()> {
    conn.execute(
        r#"
        UPDATE ownership_utxos
        SET spent_txid = ?3, spent_height = ?4, spent_tx_index = ?5
        WHERE reg_txid = ?1 AND reg_vout = ?2 AND spent_txid IS NULL
        "#,
        params![
            reg_txid,
            reg_vout as i64,
            spent_txid,
            spent_height as i64,
            spent_tx_index as i64
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

    fn list_unspent_ownership_utxos_by_outpoint(
        &self,
        reg_txid: &str,
        reg_vout: u32,
    ) -> Result<Vec<OwnershipUtxo>> {
        Ok(db_list_unspent_ownership_utxos_by_outpoint(
            &self.conn, reg_txid, reg_vout,
        )?)
    }

    fn list_unspent_ownership_ranges_by_outpoint(
        &self,
        reg_txid: &str,
        reg_vout: u32,
    ) -> Result<Vec<OwnershipRangeWithGroup>> {
        Ok(db_list_unspent_ownership_ranges_by_outpoint(
            &self.conn, reg_txid, reg_vout,
        )?)
    }

    fn list_ownership_ranges(&self, utxo: &OwnershipUtxo) -> Result<Vec<OwnershipRange>> {
        Ok(db_list_ownership_ranges(&self.conn, utxo)?)
    }

    fn find_unspent_ownership_utxo_for_slot(
        &self,
        collection_id: &CollectionKey,
        base_h160: H160,
        slot: u128,
    ) -> Result<Option<OwnershipUtxo>> {
        Ok(db_find_unspent_ownership_utxo_for_slot(
            &self.conn,
            collection_id,
            base_h160,
            slot,
        )?)
    }

    fn list_unspent_ownership_utxos_by_owner(
        &self,
        owner_h160: H160,
    ) -> Result<Vec<OwnershipUtxo>> {
        Ok(db_list_unspent_ownership_utxos_by_owner(
            &self.conn, owner_h160,
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

    fn save_ownership_utxo(&self, utxo: OwnershipUtxoSave<'_>) -> Result<()> {
        Ok(db_save_ownership_utxo(&self.conn, utxo)?)
    }

    fn save_ownership_range(
        &self,
        reg_txid: &str,
        reg_vout: u32,
        collection_id: &CollectionKey,
        base_h160: H160,
        slot_start: u128,
        slot_end: u128,
    ) -> Result<()> {
        Ok(db_save_ownership_range(
            &self.conn,
            reg_txid,
            reg_vout,
            collection_id,
            base_h160,
            slot_start,
            slot_end,
        )?)
    }

    fn mark_ownership_utxo_spent(
        &self,
        reg_txid: &str,
        reg_vout: u32,
        spent_txid: &str,
        spent_height: u64,
        spent_tx_index: u32,
    ) -> Result<()> {
        Ok(db_mark_ownership_utxo_spent(
            &self.conn,
            reg_txid,
            reg_vout,
            spent_txid,
            spent_height,
            spent_tx_index,
        )?)
    }
}

impl Storage for SqliteStorage {
    type Tx = SqliteTx;

    fn begin_tx(&self) -> Result<Self::Tx> {
        let conn = Connection::open(&self.path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
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

    pub fn load_unspent_ownership_utxos_with_ranges_by_outpoint(
        &self,
        reg_txid: &str,
        reg_vout: u32,
    ) -> Result<Vec<(OwnershipUtxo, Vec<OwnershipRange>)>> {
        Ok(self.with_conn(|conn| {
            let utxos = db_list_unspent_ownership_utxos_by_outpoint(conn, reg_txid, reg_vout)?;
            let mut out = Vec::with_capacity(utxos.len());
            for utxo in utxos {
                let ranges = db_list_ownership_ranges(conn, &utxo)?;
                out.push((utxo, ranges));
            }
            Ok(out)
        })?)
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
        conn.pragma_update(None, "foreign_keys", "ON")?;
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
            CREATE TABLE ownership_utxos (
                reg_txid TEXT NOT NULL,
                reg_vout INTEGER NOT NULL CHECK (reg_vout >= 0),
                collection_id TEXT NOT NULL,
                owner_h160 TEXT NOT NULL,
                owner_script_pubkey BLOB NOT NULL,
                base_h160 TEXT NOT NULL,
                created_height INTEGER NOT NULL CHECK (created_height >= 0),
                created_tx_index INTEGER NOT NULL CHECK (created_tx_index >= 0),
                spent_txid TEXT,
                spent_height INTEGER CHECK (spent_height IS NULL OR spent_height >= 0),
                spent_tx_index INTEGER CHECK (spent_tx_index IS NULL OR spent_tx_index >= 0),
                PRIMARY KEY (reg_txid, reg_vout, collection_id, base_h160),
                CHECK (
                    (spent_txid IS NULL AND spent_height IS NULL AND spent_tx_index IS NULL)
                    OR (spent_txid IS NOT NULL AND spent_height IS NOT NULL AND spent_tx_index IS NOT NULL)
                )
            );
            CREATE TABLE ownership_ranges (
                reg_txid TEXT NOT NULL,
                reg_vout INTEGER NOT NULL CHECK (reg_vout >= 0),
                collection_id TEXT NOT NULL,
                base_h160 TEXT NOT NULL,
                slot_start BLOB NOT NULL CHECK (length(slot_start) = 12),
                slot_end BLOB NOT NULL CHECK (length(slot_end) = 12),
                range_seq INTEGER NOT NULL CHECK (range_seq >= 0),
                PRIMARY KEY (reg_txid, reg_vout, collection_id, base_h160, slot_start, slot_end),
                CHECK (slot_start <= slot_end),
                FOREIGN KEY (reg_txid, reg_vout, collection_id, base_h160)
                    REFERENCES ownership_utxos(reg_txid, reg_vout, collection_id, base_h160)
                    ON DELETE CASCADE
            );
            CREATE INDEX ownership_utxos_unspent_owner_idx
                ON ownership_utxos(owner_h160)
                WHERE spent_txid IS NULL;
            CREATE INDEX ownership_utxos_unspent_collection_base_idx
                ON ownership_utxos(collection_id, base_h160)
                WHERE spent_txid IS NULL;
            CREATE INDEX ownership_ranges_group_idx
                ON ownership_ranges(reg_txid, reg_vout, collection_id, base_h160);
            CREATE INDEX ownership_ranges_order_idx
                ON ownership_ranges(reg_txid, reg_vout, range_seq);
        "#,
            )?;
            conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
            log::info!("🗄️ Initialized SQLite schema to v{}", DB_SCHEMA_VERSION);
            return Ok(());
        }

        log::warn!(
            "🗄️ SQLite schema version mismatch (found v{}, expected v{}); please run with --reset option",
            version,
            DB_SCHEMA_VERSION
        );

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

    fn list_unspent_ownership_utxos_by_outpoint(
        &self,
        reg_txid: &str,
        reg_vout: u32,
    ) -> Result<Vec<OwnershipUtxo>> {
        let rows = self.with_conn(|conn| {
            db_list_unspent_ownership_utxos_by_outpoint(conn, reg_txid, reg_vout)
        })?;
        Ok(rows)
    }

    fn list_unspent_ownership_ranges_by_outpoint(
        &self,
        reg_txid: &str,
        reg_vout: u32,
    ) -> Result<Vec<OwnershipRangeWithGroup>> {
        let rows = self.with_conn(|conn| {
            db_list_unspent_ownership_ranges_by_outpoint(conn, reg_txid, reg_vout)
        })?;
        Ok(rows)
    }

    fn list_ownership_ranges(&self, utxo: &OwnershipUtxo) -> Result<Vec<OwnershipRange>> {
        let rows = self.with_conn(|conn| db_list_ownership_ranges(conn, utxo))?;
        Ok(rows)
    }

    fn find_unspent_ownership_utxo_for_slot(
        &self,
        collection_id: &CollectionKey,
        base_h160: H160,
        slot: u128,
    ) -> Result<Option<OwnershipUtxo>> {
        let row = self.with_conn(|conn| {
            db_find_unspent_ownership_utxo_for_slot(conn, collection_id, base_h160, slot)
        })?;
        Ok(row)
    }

    fn list_unspent_ownership_utxos_by_owner(
        &self,
        owner_h160: H160,
    ) -> Result<Vec<OwnershipUtxo>> {
        let rows =
            self.with_conn(|conn| db_list_unspent_ownership_utxos_by_owner(conn, owner_h160))?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite::DB_SCHEMA_VERSION;
    use bitcoin::hashes::Hash;
    use bitcoin::{PubkeyHash, ScriptBuf};
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
    fn sqlite_loads_ownership_utxo_with_ranges_by_outpoint() {
        let path = unique_temp_file("brc721_ownership_outpoint", "db");
        let repo = SqliteStorage::new(&path);
        repo.init().unwrap();

        let tx = repo.begin_tx().unwrap();
        let collection_id = CollectionKey::new(840_000, 2);
        let owner_h160 = H160::from_str("0x00112233445566778899aabbccddeeff00112233").unwrap();
        let owner_script = ScriptBuf::new_p2pkh(&PubkeyHash::hash(b"owner"));
        let base_h160 = H160::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();

        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160,
            owner_script_pubkey: owner_script.as_bytes(),
            base_h160,
            reg_txid: "txid_a",
            reg_vout: 1,
            created_height: 840_001,
            created_tx_index: 3,
        })
        .unwrap();
        tx.save_ownership_range("txid_a", 1, &collection_id, base_h160, 0, 9)
            .unwrap();
        tx.save_ownership_range("txid_a", 1, &collection_id, base_h160, 10, 19)
            .unwrap();

        tx.save_ownership_utxo(OwnershipUtxoSave {
            collection_id: &collection_id,
            owner_h160,
            owner_script_pubkey: owner_script.as_bytes(),
            base_h160,
            reg_txid: "txid_b",
            reg_vout: 2,
            created_height: 840_002,
            created_tx_index: 4,
        })
        .unwrap();
        tx.save_ownership_range("txid_b", 2, &collection_id, base_h160, 42, 42)
            .unwrap();
        tx.commit().unwrap();

        let entries_a = repo
            .load_unspent_ownership_utxos_with_ranges_by_outpoint("txid_a", 1)
            .unwrap();
        assert_eq!(entries_a.len(), 1);
        assert_eq!(entries_a[0].0.collection_id, collection_id);
        assert_eq!(entries_a[0].0.owner_h160, owner_h160);
        assert_eq!(entries_a[0].0.owner_script_pubkey, owner_script.as_bytes());
        assert_eq!(entries_a[0].0.base_h160, base_h160);
        assert_eq!(entries_a[0].1.len(), 2);

        let entries_b = repo
            .load_unspent_ownership_utxos_with_ranges_by_outpoint("txid_b", 2)
            .unwrap();
        assert_eq!(entries_b.len(), 1);
        assert_eq!(entries_b[0].1.len(), 1);

        let none = repo
            .load_unspent_ownership_utxos_with_ranges_by_outpoint("txid_a", 0)
            .unwrap();
        assert!(none.is_empty());
    }
}
