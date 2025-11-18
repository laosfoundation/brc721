use ethereum_types::H160;

use crate::types::Brc721Error;

use super::Brc721Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterCollectionMessage {
    pub evm_collection_address: H160,
    pub rebaseable: bool,
}

pub type RegisterCollectionTx = [u8; 22];

impl RegisterCollectionMessage {
    pub const LEN: usize = 1 + 20 + 1;

    pub fn encode(&self) -> RegisterCollectionTx {
        let mut arr = [0u8; Self::LEN];
        arr[0] = Brc721Command::RegisterCollection as u8;
        arr[1..21].copy_from_slice(self.evm_collection_address.as_bytes());
        arr[21] = if self.rebaseable { 1 } else { 0 };
        arr
    }

    pub fn decode<T: AsRef<[u8]>>(tx: T) -> Result<Self, Brc721Error> {
        let tx = tx.as_ref();
        if tx.len() < Self::LEN {
            return Err(Brc721Error::ScriptTooShort);
        }
        if tx[0] != Brc721Command::RegisterCollection as u8 {
            return Err(Brc721Error::UnknownCommand(tx[0]));
        }
        let evm_collection_address = H160::from_slice(&tx[1..21]);
        let rebase_flag = tx[21];
        let rebaseable = match rebase_flag {
            0 => false,
            1 => true,
            other => return Err(Brc721Error::InvalidRebaseFlag(other)),
        };
        Ok(RegisterCollectionMessage {
            evm_collection_address,
            rebaseable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn addr() -> H160 {
        H160::from_str("ffff0123ffffffffffffffffffffffff3210ffff").unwrap()
    }

    #[test]
    fn encode_decode_no_rebase() {
        let m = RegisterCollectionMessage {
            evm_collection_address: addr(),
            rebaseable: false,
        };
        let arr = m.encode();
        assert_eq!(arr.len(), RegisterCollectionMessage::LEN);
        assert_eq!(arr[0], Brc721Command::RegisterCollection as u8);
        assert_eq!(&arr[1..21], m.evm_collection_address.as_bytes());
        assert_eq!(arr[21], 0);
        let dec = RegisterCollectionMessage::decode(arr).unwrap();
        assert_eq!(dec, m);
    }

    #[test]
    fn encode_decode_rebase_true() {
        let m = RegisterCollectionMessage {
            evm_collection_address: addr(),
            rebaseable: true,
        };
        let arr = m.encode();
        assert_eq!(arr[21], 1);
        let dec = RegisterCollectionMessage::decode(arr).unwrap();
        assert_eq!(dec, m);
    }

    #[test]
    fn decode_wrong_command() {
        let mut arr = RegisterCollectionMessage {
            evm_collection_address: addr(),
            rebaseable: true,
        }
        .encode();
        arr[0] = 0xFF;
        let e = RegisterCollectionMessage::decode(arr).unwrap_err();
        match e {
            Brc721Error::UnknownCommand(b) => assert_eq!(b, 0xFF),
            _ => panic!(),
        }
    }

    #[test]
    fn decode_script_too_short() {
        let bytes = &RegisterCollectionMessage {
            evm_collection_address: addr(),
            rebaseable: false,
        }
        .encode()[..RegisterCollectionMessage::LEN - 1];
        let e = RegisterCollectionMessage::decode(bytes).unwrap_err();
        match e {
            Brc721Error::ScriptTooShort => {}
            _ => panic!(),
        }
    }

    #[test]
    fn decode_invalid_rebase_flag() {
        let mut arr = RegisterCollectionMessage {
            evm_collection_address: addr(),
            rebaseable: true,
        }
        .encode();
        arr[21] = 2;
        let e = RegisterCollectionMessage::decode(arr).unwrap_err();
        match e {
            Brc721Error::InvalidRebaseFlag(b) => assert_eq!(b, 2),
            _ => panic!(),
        }
    }

    #[test]
    fn test_encode_array_bytes() {
        let msg = RegisterCollectionMessage {
            evm_collection_address: addr(),
            rebaseable: true,
        };
        let bytes = msg.encode();
        assert_eq!(
            hex::encode(bytes),
            "00ffff0123ffffffffffffffffffffffff3210ffff01"
        );
    }
}
