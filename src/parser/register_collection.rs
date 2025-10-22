use crate::types::{Brc721Tx, RegisterCollectionMessage};

use super::Brc721Error;

pub fn digest(tx: &Brc721Tx) -> Result<(), Brc721Error> {
    let payload = parse(tx)?;
    log::info!("📝 RegisterCollectionMessage: {:?}", payload);
    Ok(())
}

fn parse(tx: &Brc721Tx) -> Result<RegisterCollectionMessage, Brc721Error> {
    use crate::types::MessageDecodeError;
    match RegisterCollectionMessage::decode(tx) {
        Ok(msg) => Ok(msg),
        Err(MessageDecodeError::ScriptTooShort) => Err(Brc721Error::ScriptTooShort),
        Err(MessageDecodeError::WrongCommand(b)) => Err(Brc721Error::WrongCommand(b)),
        Err(MessageDecodeError::InvalidRebaseFlag(b)) => Err(Brc721Error::InvalidRebaseFlag(b)),
    }
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

    #[test]
    fn test_encode_array_round_trip() {
        let msg = RegisterCollectionMessage {
            collection_address: CollectionAddress::from_str(
                "ffff0123ffffffffffffffffffffffff3210ffff",
            )
            .unwrap(),
            rebaseable: false,
        };
        let arr = msg.encode();
        let decoded = RegisterCollectionMessage::decode(arr).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_round_trip_encode_decode() {
        let msg = RegisterCollectionMessage {
            collection_address: CollectionAddress::from_str(
                "ffff0123ffffffffffffffffffffffff3210ffff",
            )
            .unwrap(),
            rebaseable: true,
        };
        let bytes = msg.encode();
        let decoded = RegisterCollectionMessage::decode(bytes).unwrap();
        assert_eq!(decoded, msg);
    }
}
