use bdk_wallet::keys::bip39::Mnemonic;
use std::path::PathBuf;

pub struct InitResult {
    pub created: bool,
    pub mnemonic: Option<Mnemonic>,
    pub db_path: PathBuf,
}
