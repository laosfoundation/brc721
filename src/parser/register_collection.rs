use crate::types::{Brc721Tx, RegisterCollectionMessage};

use super::Brc721Error;

pub fn digest(tx: &Brc721Tx) -> Result<(), Brc721Error> {
    let payload = parse(tx)?;
    log::info!("📝 RegisterCollectionMessage: {:?}", payload);
    Ok(())
}

fn parse(tx: &Brc721Tx) -> Result<RegisterCollectionMessage, Brc721Error> {
    Ok(RegisterCollectionMessage::decode(tx)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionAddress;
    use std::str::FromStr;

    #[test]
    fn test_parse_register_collection_no_rebaseable() {
        let msg = RegisterCollectionMessage {
            collection_address: CollectionAddress::from_str(
                "ffff0123ffffffffffffffffffffffff3210ffff",
            )
            .unwrap(),
            rebaseable: false,
        }
        .encode();

        let register_collection = parse(&msg).unwrap();
        assert_eq!(
            register_collection.collection_address,
            CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
        );
        assert!(!register_collection.rebaseable)
    }

    #[test]
    fn test_parse_register_collection_rebaseable() {
        let msg = RegisterCollectionMessage {
            collection_address: CollectionAddress::from_str(
                "ffff0123ffffffffffffffffffffffff3210ffff",
            )
            .unwrap(),
            rebaseable: true,
        }
        .encode();

        let register_collection = parse(&msg).unwrap();
        assert_eq!(
            register_collection.collection_address,
            CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
        );
        assert!(register_collection.rebaseable)
    }
}
