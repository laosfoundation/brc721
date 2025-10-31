use crate::types::{Brc721Tx, RegisterCollectionMessage};

use super::Brc721Error;

pub fn digest(tx: &Brc721Tx) -> Result<(), Brc721Error> {
    let payload = RegisterCollectionMessage::decode(tx)?;
    log::info!("📝 RegisterCollectionMessage: {:?}", payload);
    Ok(())
}
