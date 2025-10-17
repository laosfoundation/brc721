use bitcoin::blockdata::opcodes::all as opcodes;
use bitcoin::ScriptBuf;

use crate::types::{Brc721Command, CollectionAddress, RegisterCollectionPayload, BRC721_CODE};

use super::Brc721Error;

pub fn digest(script: &ScriptBuf) -> Result<(), Brc721Error> {
    let payload = parse(script)?;
    log::info!("📝 RegisterCollectionPayload: {:?}", payload);
    Ok(())
}

fn parse(script: &ScriptBuf) -> Result<RegisterCollectionPayload, Brc721Error> {
    let bytes = script.clone().into_bytes();

    if bytes.len() < 1 + 1 + 1 + 20 + 1 {
        return Err(Brc721Error::ScriptTooShort);
    }

    if bytes[0] != opcodes::OP_RETURN.to_u8() {
        return Err(Brc721Error::NotOpReturn);
    }

    if bytes[1] != BRC721_CODE {
        return Err(Brc721Error::WrongProtocolCode {
            expected: BRC721_CODE,
            found: bytes[1],
        });
    }

    if bytes[2] != Brc721Command::CreateCollection as u8 {
        return Err(Brc721Error::WrongCommand {
            expected: Brc721Command::CreateCollection as u8,
            found: bytes[2],
        });
    }

    let addr_bytes = &bytes[3..23];
    let collection_address = CollectionAddress::from_slice(addr_bytes);

    let rebase_flag = bytes[23];
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
        let script =
            ScriptBuf::from_hex("6a5f00ffff0123ffffffffffffffffffffffff3210ffff00").unwrap();

        let register_collection = parse(&script).unwrap();
        assert_eq!(
            register_collection.collection_address,
            CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
        );
        assert!(!register_collection.rebaseable)
    }

    #[test]
    fn test_parse_register_collection_rebaseable() {
        let script =
            ScriptBuf::from_hex("6a5f00ffff0123ffffffffffffffffffffffff3210ffff01").unwrap();

        let register_collection = parse(&script).unwrap();
        assert_eq!(
            register_collection.collection_address,
            CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
        );
        assert!(register_collection.rebaseable)
    }
}
