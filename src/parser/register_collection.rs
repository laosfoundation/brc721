use crate::storage::Storage;
use crate::types::{Brc721Tx, RegisterCollectionMessage};

use super::Brc721Error;

pub fn digest(tx: &Brc721Tx, _storage: &dyn Storage) -> Result<(), Brc721Error> {
    let _payload = RegisterCollectionMessage::decode(tx)?;
    Ok(())
}
