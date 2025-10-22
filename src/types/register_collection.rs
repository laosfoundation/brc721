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

    pub fn encode_array(&self) -> RegisterCollectionTx {
        let mut arr = [0u8; Self::LEN];
        arr[0] = Brc721Command::RegisterCollection as u8;
        arr[1..21].copy_from_slice(self.collection_address.as_bytes());
        arr[21] = if self.rebaseable { 1 } else { 0 };
        arr
    }

    pub fn encode(&self) -> Vec<u8> {
        self.encode_array().to_vec()
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
