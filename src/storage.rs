use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub trait Storage {
    fn load_last_height(&self) -> io::Result<Option<u64>>;
    fn save_last_height(&self, height: u64) -> io::Result<()>;
}

#[derive(Clone, Debug)]
pub struct FileStorage {
    path: PathBuf,
}

impl FileStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self { path: path.as_ref().to_path_buf() }
    }
}

impl Storage for FileStorage {
    fn load_last_height(&self) -> io::Result<Option<u64>> {
        match fs::read_to_string(&self.path) {
            Ok(s) => {
                let t = s.trim();
                match t.parse::<u64>() {
                    Ok(n) => Ok(Some(n)),
                    Err(_) => Ok(None),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn save_last_height(&self, height: u64) -> io::Result<()> {
        let dir = self.path.parent();
        if let Some(d) = dir {
            if !d.as_os_str().is_empty() {
                fs::create_dir_all(d)?;
            }
        }
        fs::write(&self.path, height.to_string().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_file() -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        p.push(format!("brc721_state_{}.txt", nanos));
        p
    }

    #[test]
    fn load_returns_none_when_missing() {
        let path = unique_temp_file();
        let store = FileStorage::new(&path);
        let v = store.load_last_height().unwrap();
        assert!(v.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let path = unique_temp_file();
        let store = FileStorage::new(&path);
        store.save_last_height(42).unwrap();
        let v = store.load_last_height().unwrap();
        assert_eq!(v, Some(42));
    }
}
