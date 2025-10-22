use super::{Brc721Command, CollectionAddress};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterCollectionMessage {
    pub collection_address: CollectionAddress,
    pub rebaseable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageDecodeError {
    ScriptTooShort,
    WrongCommand(u8),
    InvalidRebaseFlag(u8),
}

pub type RegisterCollectionTx = [u8; 22];

impl RegisterCollectionMessage {
    pub const LEN: usize = 1 + 20 + 1;

    pub fn encode(&self) -> RegisterCollectionTx {
        let mut arr = [0u8; Self::LEN];
        arr[0] = Brc721Command::RegisterCollection as u8;
        arr[1..21].copy_from_slice(self.collection_address.as_bytes());
        arr[21] = if self.rebaseable { 1 } else { 0 };
        arr
    }

    pub fn decode<T: AsRef<[u8]>>(tx: T) -> Result<Self, MessageDecodeError> {
        let tx = tx.as_ref();
        if tx.len() < Self::LEN {
            return Err(MessageDecodeError::ScriptTooShort);
        }
        if tx[0] != Brc721Command::RegisterCollection as u8 {
            return Err(MessageDecodeError::WrongCommand(tx[0]));
        }
        let collection_address = CollectionAddress::from_slice(&tx[1..21]);
        let rebase_flag = tx[21];
        let rebaseable = match rebase_flag {
            0 => false,
            1 => true,
            other => return Err(MessageDecodeError::InvalidRebaseFlag(other)),
        };
        Ok(RegisterCollectionMessage {
            collection_address,
            rebaseable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn addr() -> CollectionAddress {
        CollectionAddress::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
    }

    #[test]
    fn encode_decode_no_rebase() {
        let m = RegisterCollectionMessage { collection_address: addr(), rebaseable: false };
        let arr = m.encode();
        assert_eq!(arr.len(), RegisterCollectionMessage::LEN);
        assert_eq!(arr[0], Brc721Command::RegisterCollection as u8);
        assert_eq!(&arr[1..21], m.collection_address.as_bytes());
        assert_eq!(arr[21], 0);
        let dec = RegisterCollectionMessage::decode(arr).unwrap();
        assert_eq!(dec, m);
    }

    #[test]
    fn encode_decode_rebase_true() {
        let m = RegisterCollectionMessage { collection_address: addr(), rebaseable: true };
        let arr = m.encode();
        assert_eq!(arr[21], 1);
        let dec = RegisterCollectionMessage::decode(arr).unwrap();
        assert_eq!(dec, m);
    }

    #[test]
    fn decode_wrong_command() {
        let mut arr = RegisterCollectionMessage { collection_address: addr(), rebaseable: true }.encode();
        arr[0] = 0xFF;
        let e = RegisterCollectionMessage::decode(arr).unwrap_err();
        match e { MessageDecodeError::WrongCommand(b) => assert_eq!(b, 0xFF), _ => panic!() }
    }

    #[test]
    fn decode_script_too_short() {
        let bytes = &RegisterCollectionMessage { collection_address: addr(), rebaseable: false }.encode()[..RegisterCollectionMessage::LEN-1];
        let e = RegisterCollectionMessage::decode(bytes).unwrap_err();
        match e { MessageDecodeError::ScriptTooShort => {}, _ => panic!() }
    }

    #[test]
    fn decode_invalid_rebase_flag() {
        let mut arr = RegisterCollectionMessage { collection_address: addr(), rebaseable: true }.encode();
        arr[21] = 2;
        let e = RegisterCollectionMessage::decode(arr).unwrap_err();
        match e { MessageDecodeError::InvalidRebaseFlag(b) => assert_eq!(b, 2), _ => panic!() }
    }
}
