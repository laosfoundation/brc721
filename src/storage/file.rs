use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{CollectionRow, LastBlock, Storage};

#[derive(Clone, Debug)]
pub struct FileStorage {
    path: PathBuf,
}

impl FileStorage {
    #[allow(dead_code)]
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl Storage for FileStorage {
    fn load_last(&self) -> io::Result<Option<LastBlock>> {
        match fs::read_to_string(&self.path) {
            Ok(s) => {
                let t = s.trim();
                let mut it = t.split_whitespace();
                let h = it.next();
                let hh = it.next();
                match (h, hh) {
                    (Some(hs), Some(hash)) => match hs.parse::<u64>() {
                        Ok(height) => Ok(Some(LastBlock {
                            height,
                            hash: hash.to_string(),
                        })),
                        Err(_) => Ok(None),
                    },
                    _ => Ok(None),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn save_last(&self, height: u64, hash: &str) -> io::Result<()> {
        let dir = self.path.parent();
        if let Some(d) = dir {
            if !d.as_os_str().is_empty() {
                fs::create_dir_all(d)?;
            }
        }
        let s = format!("{} {}\n", height, hash);
        fs::write(&self.path, s.as_bytes())
    }

    fn insert_collections_batch(&self, _rows: &[CollectionRow]) -> rusqlite::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_file(prefix: &str, ext: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("{}_{}.{}", prefix, nanos, ext));
        p
    }

    #[test]
    fn file_storage_load_returns_none_when_missing() {
        let path = unique_temp_file("brc721_state", "txt");
        let store = FileStorage::new(&path);
        let v = store.load_last().unwrap();
        assert!(v.is_none());
    }

    #[test]
    fn file_storage_save_and_load_roundtrip() {
        let path = unique_temp_file("brc721_state", "txt");
        let store = FileStorage::new(&path);
        store.save_last(42, "deadbeef").unwrap();
        let v = store.load_last().unwrap();
        assert_eq!(
            v,
            Some(LastBlock {
                height: 42,
                hash: "deadbeef".to_string()
            })
        );
    }
}
