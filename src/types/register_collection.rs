use crate::types::Brc721Error;
use ethereum_types::H160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterCollectionData {
    pub evm_collection_address: H160,
    pub rebaseable: bool,
}

impl RegisterCollectionData {
    pub const LEN: usize = 20 + 1;

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend_from_slice(self.evm_collection_address.as_bytes());
        out.push(u8::from(self.rebaseable));
        out
    }
}

impl TryFrom<&[u8]> for RegisterCollectionData {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != Self::LEN {
            return Err(Brc721Error::InvalidLength(Self::LEN, bytes.len()));
        }
        let evm_collection_address = H160::from_slice(&bytes[0..20]);
        let rebase_flag = bytes[20];
        let rebaseable = match rebase_flag {
            0 => false,
            1 => true,
            other => return Err(Brc721Error::InvalidRebaseFlag(other)),
        };
        Ok(RegisterCollectionData {
            evm_collection_address,
            rebaseable,
        })
    }
}
