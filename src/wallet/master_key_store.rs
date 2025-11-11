use std::fs::{self, File};
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};

use age::secrecy::SecretString;
use age::Encryptor;
use age::{scrypt, Decryptor};
use anyhow::{bail, Context, Result};
use bitcoin::bip32::Xpriv;
use std::str::FromStr;

/// Stores and retrieves a master private key (Xpriv) encrypted with the age crate.
pub struct MasterKeyStore {
    path: PathBuf,
}

impl MasterKeyStore {
    /// Create a new store with filename depending on network
    pub fn new<P: AsRef<Path>>(data_dir: P) -> Self {
        let mut path = PathBuf::from(data_dir.as_ref());
        path.push("master-key.age");
        Self { path }
    }

    /// Store the provided Xpriv. Fails if a key is already stored.
    pub fn store(&self, xpriv: &Xpriv, passphrase: &SecretString) -> Result<()> {
        if self.path.exists() {
            bail!("master key already stored");
        }
        let encoded = xpriv.to_string();
        self.persist_encrypted(passphrase, encoded.as_bytes())
    }

    /// Load the stored Xpriv by decrypting it with the provided passphrase.
    pub fn load(&self, passphrase: &SecretString) -> Result<Xpriv> {
        let plaintext = self.decrypt_bytes(passphrase)?;
        let s = String::from_utf8(plaintext).context("xpriv plaintext not valid utf-8")?;
        let x = Xpriv::from_str(&s).context("parsing xpriv")?;
        Ok(x)
    }

    /// Decrypt and return the existing master key bytes.
    fn decrypt_bytes(&self, passphrase: &SecretString) -> Result<Vec<u8>> {
        let ciphertext = fs::read(&self.path).with_context(|| {
            format!("reading encrypted master key from {}", self.path.display())
        })?;

        let decryptor = Decryptor::new(&ciphertext[..]).context("creating age decryptor")?;
        if !decryptor.is_scrypt() {
            bail!("encrypted master key is not scrypt/passphrase protected");
        }
        let identity = scrypt::Identity::new(passphrase.clone());
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .context("decrypting master key")?;
        let mut pt = Vec::new();
        reader
            .read_to_end(&mut pt)
            .context("reading decrypted key")?;
        Ok(pt)
    }

    fn persist_encrypted(&self, passphrase: &SecretString, key: &[u8]) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).ok();
        }
        let encryptor = Encryptor::with_user_passphrase(passphrase.clone());
        let mut out = Vec::new();
        {
            let mut writer = encryptor
                .wrap_output(&mut out)
                .context("wrapping age output")?;
            writer.write_all(key).context("writing plaintext key")?;
            writer.finish().context("finishing encryption")?;
        }
        let mut file = File::create(&self.path)
            .with_context(|| format!("creating {}", self.path.display()))?;
        file.write_all(&out)
            .with_context(|| format!("writing {}", self.path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::Network;
    use tempfile::TempDir;

    #[test]
    fn store_then_load_xpriv_roundtrip() {
        let dir = TempDir::new().unwrap();
        let passphrase = SecretString::from("test-passphrase".to_string());
        let store = MasterKeyStore::new(dir.path());

        // User-provided master key
        let seed = [7u8; 32];
        let xpriv = Xpriv::new_master(Network::Regtest, &seed).expect("xpriv");

        // First store succeeds
        store.store(&xpriv, &passphrase).expect("store");
        // Second store should fail because it already exists
        assert!(store.store(&xpriv, &passphrase).is_err());

        // Load back
        let loaded = store.load(&passphrase).expect("load");
        assert_eq!(loaded, xpriv);
    }
}
