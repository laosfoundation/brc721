use crate::types::Brc721Error;
use ethereum_types::H160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterCollectionData {
    pub evm_collection_address: H160,
    pub rebaseable: bool,
}

impl RegisterCollectionData {
    pub const ADDR_LEN: usize = 20;
    pub const LEN: usize = Self::ADDR_LEN + 1;

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
        let evm_collection_address = H160::from_slice(&bytes[0..Self::ADDR_LEN]);
        let rebase_flag = bytes[Self::ADDR_LEN];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_register_collection_data() {
        let addr = H160::from_low_u64_be(42);
        let data = RegisterCollectionData {
            evm_collection_address: addr,
            rebaseable: true,
        };

        let bytes = data.to_bytes();
        assert_eq!(bytes.len(), RegisterCollectionData::LEN);

        let parsed = RegisterCollectionData::try_from(bytes.as_slice()).unwrap();
        assert_eq!(parsed.evm_collection_address, addr);
        assert_eq!(parsed.rebaseable, true);
    }

    #[test]
    fn reject_invalid_length() {
        let too_short = vec![0u8; RegisterCollectionData::LEN - 1];
        assert!(RegisterCollectionData::try_from(too_short.as_slice()).is_err());

        let too_long = vec![0u8; RegisterCollectionData::LEN + 1];
        assert!(RegisterCollectionData::try_from(too_long.as_slice()).is_err());
    }

    #[test]
    fn reject_invalid_rebase_flag() {
        let addr = H160::from_low_u64_be(1);
        let mut bytes = Vec::with_capacity(RegisterCollectionData::LEN);
        bytes.extend_from_slice(addr.as_bytes());
        bytes.push(2); // invalid flag

        match RegisterCollectionData::try_from(bytes.as_slice()) {
            Err(Brc721Error::InvalidRebaseFlag(2)) => {}
            other => panic!("expected InvalidRebaseFlag(2), got {:?}", other),
        }
    }
}
