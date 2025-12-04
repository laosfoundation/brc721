#![allow(dead_code)]

use crate::types::Brc721Error;
use ethereum_types::{H160, U256};

/// Represents a 256-bit BRC721 TokenID composed of a 96-bit slot number and a 160-bit H160 address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Brc721Token {
    slot_number: u128,
    h160_address: H160,
}

impl Brc721Token {
    pub const SLOT_BITS: usize = 96;
    pub const ADDRESS_BITS: usize = 160;
    pub const SLOT_BYTES: usize = Self::SLOT_BITS / 8;
    pub const ADDRESS_BYTES: usize = Self::ADDRESS_BITS / 8;
    pub const LEN: usize = Self::SLOT_BYTES + Self::ADDRESS_BYTES;
    pub const MAX_SLOT: u128 = (1u128 << Self::SLOT_BITS) - 1;

    pub fn new(slot_number: u128, h160_address: H160) -> Result<Self, Brc721Error> {
        if slot_number > Self::MAX_SLOT {
            return Err(Brc721Error::InvalidSlotNumber(slot_number));
        }
        Ok(Self {
            slot_number,
            h160_address,
        })
    }

    pub fn slot_number(&self) -> u128 {
        self.slot_number
    }

    pub fn h160_address(&self) -> H160 {
        self.h160_address
    }

    pub fn to_u256(&self) -> U256 {
        let slot = U256::from(self.slot_number) << Self::ADDRESS_BITS;
        let addr = U256::from_big_endian(self.h160_address.as_bytes());
        slot | addr
    }

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        self.to_u256().to_big_endian()
    }
}

impl TryFrom<U256> for Brc721Token {
    type Error = Brc721Error;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        let slot_part = value >> Self::ADDRESS_BITS;
        let slot_number = slot_part.low_u128();

        let addr_mask = (U256::one() << Self::ADDRESS_BITS) - U256::one();
        let addr_part = value & addr_mask;
        let addr_bytes = addr_part.to_big_endian();
        let mut h160_raw = [0u8; Self::ADDRESS_BYTES];
        h160_raw.copy_from_slice(&addr_bytes[Self::SLOT_BYTES..]);
        let h160_address = H160::from(h160_raw);

        Brc721Token::new(slot_number, h160_address)
    }
}

impl TryFrom<&[u8]> for Brc721Token {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != Self::LEN {
            return Err(Brc721Error::InvalidLength(Self::LEN, bytes.len()));
        }
        let value = U256::from_big_endian(bytes);
        Brc721Token::try_from(value)
    }
}

impl TryFrom<[u8; 32]> for Brc721Token {
    type Error = Brc721Error;

    fn try_from(bytes: [u8; 32]) -> Result<Self, Self::Error> {
        Brc721Token::try_from(bytes.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_address() -> H160 {
        H160::from([
            0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99,
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ])
    }

    #[test]
    fn new_rejects_slot_overflow() {
        let slot = Brc721Token::MAX_SLOT + 1;
        let err = Brc721Token::new(slot, sample_address()).unwrap_err();
        assert_eq!(err, Brc721Error::InvalidSlotNumber(slot));
    }

    #[test]
    fn to_u256_and_back_preserves_fields() {
        let slot: u128 = 0x0000_abcd_ef01_2345_6789_abcd;
        let addr = sample_address();
        let token = Brc721Token::new(slot, addr).expect("valid token");

        let encoded = token.to_u256();
        let decoded = Brc721Token::try_from(encoded).expect("decode should work");

        assert_eq!(decoded.slot_number(), slot);
        assert_eq!(decoded.h160_address(), addr);
    }

    #[test]
    fn to_bytes_layout_matches_spec() {
        let slot: u128 = 0x0000_1234_5678_9abc_def0_1111;
        let addr = sample_address();
        let token = Brc721Token::new(slot, addr).expect("valid token");

        let bytes = token.to_bytes();
        assert_eq!(bytes.len(), Brc721Token::LEN);

        let slot_bytes = slot.to_be_bytes();
        assert_eq!(
            &bytes[..Brc721Token::SLOT_BYTES],
            &slot_bytes[slot_bytes.len() - Brc721Token::SLOT_BYTES..]
        );
        assert_eq!(&bytes[Brc721Token::SLOT_BYTES..], addr.as_bytes());
    }

    #[test]
    fn parse_from_byte_slice_validates_length() {
        let short = [0u8; Brc721Token::LEN - 1];
        match Brc721Token::try_from(short.as_slice()) {
            Err(Brc721Error::InvalidLength(expected, actual)) => {
                assert_eq!(expected, Brc721Token::LEN);
                assert_eq!(actual, Brc721Token::LEN - 1);
            }
            other => panic!("expected InvalidLength, got {:?}", other),
        }
    }

    #[test]
    fn slice_roundtrip_matches_original() {
        let slot: u128 = 42;
        let addr = sample_address();
        let token = Brc721Token::new(slot, addr).expect("valid token");

        let bytes = token.to_bytes();
        let parsed = Brc721Token::try_from(bytes.as_slice()).expect("parse success");

        assert_eq!(parsed, token);
    }
}
