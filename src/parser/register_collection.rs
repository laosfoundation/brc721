use crate::types::{Brc721Command, Brc721Tx, CollectionAddress, RegisterCollectionPayload};

use super::Brc721Error;

pub fn digest(tx: &Brc721Tx) -> Result<(), Brc721Error> {
    let payload = parse(tx)?;
    log::info!("📝 RegisterCollectionPayload: {:?}", payload);
    Ok(())
}

fn parse(tx: &Brc721Tx) -> Result<RegisterCollectionPayload, Brc721Error> {
    let bytes = tx;

    if bytes.len() < 1 + 20 + 1 {
        return Err(Brc721Error::ScriptTooShort);
    }

    if bytes[0] != Brc721Command::RegisterCollection as u8 {
        return Err(Brc721Error::WrongCommand(bytes[2]));
    }

    let addr_bytes = &bytes[1..21];
    let collection_address = CollectionAddress::from_slice(addr_bytes);

    let rebase_flag = bytes[21];
    let rebaseable = match rebase_flag {
        0 => false,
        1 => true,
        other => return Err(Brc721Error::InvalidRebaseFlag(other)),
    };

    Ok(RegisterCollectionPayload {
        collection_address,
        rebaseable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionAddress;
    use std::str::FromStr;

    #[test]
    fn test_parse_register_collection_no_rebaseable() {
        let tx = hex::decode("00ffff0123ffffffffffffffffffffffff3210ffff00").unwrap();

        let register_collection = parse(&tx).unwrap();
        assert_eq!(
            register_collection.collection_address,
            CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
        );
        assert!(!register_collection.rebaseable)
    }

    #[test]
    fn test_parse_register_collection_rebaseable() {
        let tx = hex::decode("00ffff0123ffffffffffffffffffffffff3210ffff01").unwrap();

        let register_collection = parse(&tx).unwrap();
        assert_eq!(
            register_collection.collection_address,
            CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
        );
        assert!(register_collection.rebaseable)
    }
}
