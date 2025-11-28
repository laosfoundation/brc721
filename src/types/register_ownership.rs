use crate::types::Brc721Error;
use ethereum_types::H160;

const SLOT_NUMBER_BYTES: usize = 12;
#[cfg_attr(not(test), allow(dead_code))]
const TOKEN_ID_BYTES: usize = SLOT_NUMBER_BYTES + 20;
const SINGLE_SLOT_TAG: u8 = 0x00;
const RANGE_SLOT_TAG: u8 = 0x01;

/// Represents the `<block_height>:<tx_index>` pair that uniquely identifies the
/// transaction which registered a collection on Bitcoin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitcoinCollectionId {
    block_height: u32,
    tx_index: u32,
}

impl BitcoinCollectionId {
    pub const LEN: usize = 8;

    pub fn new(block_height: u32, tx_index: u32) -> Self {
        Self {
            block_height,
            tx_index,
        }
    }

    pub fn block_height(&self) -> u32 {
        self.block_height
    }

    pub fn tx_index(&self) -> u32 {
        self.tx_index
    }

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut buf = [0u8; Self::LEN];
        buf[..4].copy_from_slice(&self.block_height.to_be_bytes());
        buf[4..].copy_from_slice(&self.tx_index.to_be_bytes());
        buf
    }
}

impl TryFrom<&[u8]> for BitcoinCollectionId {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != Self::LEN {
            return Err(Brc721Error::InvalidLength(Self::LEN, bytes.len()));
        }
        let mut block_bytes = [0u8; 4];
        block_bytes.copy_from_slice(&bytes[..4]);
        let mut tx_bytes = [0u8; 4];
        tx_bytes.copy_from_slice(&bytes[4..]);
        Ok(Self::new(
            u32::from_be_bytes(block_bytes),
            u32::from_be_bytes(tx_bytes),
        ))
    }
}

/// 96-bit slot identifier within a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotNumber(u128);

impl SlotNumber {
    pub const MAX: u128 = (1u128 << 96) - 1;

    pub fn new(value: u128) -> Result<Self, Brc721Error> {
        if value > Self::MAX {
            return Err(Brc721Error::SlotNumberTooLarge(value));
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u128 {
        self.0
    }

    pub fn to_bytes(self) -> [u8; SLOT_NUMBER_BYTES] {
        let full = self.0.to_be_bytes();
        let mut buf = [0u8; SLOT_NUMBER_BYTES];
        buf.copy_from_slice(&full[16 - SLOT_NUMBER_BYTES..]);
        buf
    }
}

impl TryFrom<&[u8]> for SlotNumber {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != SLOT_NUMBER_BYTES {
            return Err(Brc721Error::InvalidLength(SLOT_NUMBER_BYTES, bytes.len()));
        }
        let mut padded = [0u8; 16];
        padded[16 - SLOT_NUMBER_BYTES..].copy_from_slice(bytes);
        let value = u128::from_be_bytes(padded);
        SlotNumber::new(value)
    }
}

/// Token identifier composed of the slot number (96-bit) and h160 owner bytes.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenId {
    slot_number: SlotNumber,
    h160_address: H160,
}

#[cfg_attr(not(test), allow(dead_code))]
impl TokenId {
    pub fn new(slot_number: SlotNumber, h160_address: H160) -> Self {
        Self {
            slot_number,
            h160_address,
        }
    }

    pub fn slot_number(&self) -> SlotNumber {
        self.slot_number
    }

    pub fn h160_address(&self) -> H160 {
        self.h160_address
    }

    pub fn to_bytes(&self) -> [u8; TOKEN_ID_BYTES] {
        let mut buf = [0u8; TOKEN_ID_BYTES];
        buf[..SLOT_NUMBER_BYTES].copy_from_slice(&self.slot_number.to_bytes());
        buf[SLOT_NUMBER_BYTES..].copy_from_slice(self.h160_address.as_bytes());
        buf
    }
}

impl TryFrom<&[u8]> for TokenId {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != TOKEN_ID_BYTES {
            return Err(Brc721Error::InvalidLength(TOKEN_ID_BYTES, bytes.len()));
        }
        let slot = SlotNumber::try_from(&bytes[..SLOT_NUMBER_BYTES])?;
        let addr = H160::from_slice(&bytes[SLOT_NUMBER_BYTES..]);
        Ok(Self {
            slot_number: slot,
            h160_address: addr,
        })
    }
}

/// A contiguous set of slot numbers assigned to an output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotRange {
    Single(SlotNumber),
    Interval { start: SlotNumber, end: SlotNumber },
}

impl SlotRange {
    pub fn single(slot: SlotNumber) -> Self {
        Self::Single(slot)
    }

    pub fn interval(start: SlotNumber, end: SlotNumber) -> Result<Self, Brc721Error> {
        if start >= end {
            return Err(Brc721Error::InvalidSlotRange {
                start: start.value(),
                end: end.value(),
            });
        }
        Ok(Self::Interval { start, end })
    }

    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            SlotRange::Single(slot) => {
                out.push(SINGLE_SLOT_TAG);
                out.extend_from_slice(&slot.to_bytes());
            }
            SlotRange::Interval { start, end } => {
                out.push(RANGE_SLOT_TAG);
                out.extend_from_slice(&start.to_bytes());
                out.extend_from_slice(&end.to_bytes());
            }
        }
    }

    fn decode(bytes: &[u8], cursor: &mut usize) -> Result<Self, Brc721Error> {
        let tag = bytes.get(*cursor).ok_or(Brc721Error::InvalidPayload)?;
        *cursor += 1;
        match *tag {
            SINGLE_SLOT_TAG => {
                let slot = read_slot_number(bytes, cursor)?;
                Ok(SlotRange::single(slot))
            }
            RANGE_SLOT_TAG => {
                let start = read_slot_number(bytes, cursor)?;
                let end = read_slot_number(bytes, cursor)?;
                SlotRange::interval(start, end)
            }
            other => Err(Brc721Error::InvalidSlotRangeTag(other)),
        }
    }
}

/// Associates one or more slot ranges to a specific Bitcoin output index (1-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotMapping {
    output_index: u64,
    slot_ranges: Vec<SlotRange>,
}

impl SlotMapping {
    pub fn new(output_index: u64, slot_ranges: Vec<SlotRange>) -> Result<Self, Brc721Error> {
        if output_index == 0 {
            return Err(Brc721Error::InvalidOutputIndex(output_index));
        }
        if slot_ranges.is_empty() {
            return Err(Brc721Error::EmptySlotRangeList(output_index));
        }
        Ok(Self {
            output_index,
            slot_ranges,
        })
    }

    pub fn output_index(&self) -> u64 {
        self.output_index
    }

    pub fn slot_ranges(&self) -> &[SlotRange] {
        &self.slot_ranges
    }

    fn encode(&self, out: &mut Vec<u8>) {
        write_varint(out, self.output_index);
        write_varint(out, self.slot_ranges.len() as u64);
        for range in &self.slot_ranges {
            range.encode(out);
        }
    }
}

/// Payload carried by the Register Ownership command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterOwnershipData {
    collection_id: BitcoinCollectionId,
    slot_mappings: Vec<SlotMapping>,
}

impl RegisterOwnershipData {
    pub fn new(
        collection_id: BitcoinCollectionId,
        slot_mappings: Vec<SlotMapping>,
    ) -> Result<Self, Brc721Error> {
        if slot_mappings.is_empty() {
            return Err(Brc721Error::MissingSlotMappings);
        }
        Ok(Self {
            collection_id,
            slot_mappings,
        })
    }

    pub fn collection_id(&self) -> BitcoinCollectionId {
        self.collection_id
    }

    pub fn slot_mappings(&self) -> &[SlotMapping] {
        &self.slot_mappings
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.collection_id.to_bytes());
        write_varint(&mut out, self.slot_mappings.len() as u64);
        for mapping in &self.slot_mappings {
            mapping.encode(&mut out);
        }
        out
    }
}

impl TryFrom<&[u8]> for RegisterOwnershipData {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() < BitcoinCollectionId::LEN {
            return Err(Brc721Error::InvalidPayload);
        }
        let collection_id = BitcoinCollectionId::try_from(&bytes[..BitcoinCollectionId::LEN])?;
        let mut cursor = BitcoinCollectionId::LEN;
        let mapping_count = read_varint(bytes, &mut cursor)?;
        if mapping_count == 0 {
            return Err(Brc721Error::MissingSlotMappings);
        }
        let mapping_count_usize: usize = mapping_count
            .try_into()
            .map_err(|_| Brc721Error::InvalidPayload)?;
        let mut slot_mappings = Vec::with_capacity(mapping_count_usize);
        for _ in 0..mapping_count_usize {
            let output_index = read_varint(bytes, &mut cursor)?;
            let range_count = read_varint(bytes, &mut cursor)?;
            if range_count == 0 {
                return Err(Brc721Error::EmptySlotRangeList(output_index));
            }
            let range_count_usize: usize = range_count
                .try_into()
                .map_err(|_| Brc721Error::InvalidPayload)?;
            let mut ranges = Vec::with_capacity(range_count_usize);
            for _ in 0..range_count_usize {
                let range = SlotRange::decode(bytes, &mut cursor)?;
                ranges.push(range);
            }
            let mapping = SlotMapping::new(output_index, ranges)?;
            slot_mappings.push(mapping);
        }

        if cursor != bytes.len() {
            return Err(Brc721Error::InvalidPayload);
        }

        RegisterOwnershipData::new(collection_id, slot_mappings)
    }
}

fn write_varint(out: &mut Vec<u8>, value: u64) {
    match value {
        0..=0xFC => out.push(value as u8),
        0xFD..=0xFFFF => {
            out.push(0xFD);
            out.extend_from_slice(&(value as u16).to_le_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            out.push(0xFE);
            out.extend_from_slice(&(value as u32).to_le_bytes());
        }
        _ => {
            out.push(0xFF);
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn read_varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, Brc721Error> {
    let first = *bytes.get(*cursor).ok_or(Brc721Error::InvalidPayload)?;
    *cursor += 1;
    match first {
        0x00..=0xFC => Ok(first as u64),
        0xFD => {
            let raw = read_exact(bytes, cursor, 2)?;
            Ok(u16::from_le_bytes([raw[0], raw[1]]) as u64)
        }
        0xFE => {
            let raw = read_exact(bytes, cursor, 4)?;
            Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as u64)
        }
        0xFF => {
            let raw = read_exact(bytes, cursor, 8)?;
            Ok(u64::from_le_bytes([
                raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
            ]))
        }
    }
}

fn read_exact<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], Brc721Error> {
    if bytes.len() < *cursor + len {
        return Err(Brc721Error::InvalidPayload);
    }
    let slice = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice)
}

fn read_slot_number(bytes: &[u8], cursor: &mut usize) -> Result<SlotNumber, Brc721Error> {
    let raw = read_exact(bytes, cursor, SLOT_NUMBER_BYTES)?;
    SlotNumber::try_from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_number_rejects_out_of_range() {
        let too_large = 1u128 << 96;
        let err = SlotNumber::new(too_large).unwrap_err();
        assert_eq!(err, Brc721Error::SlotNumberTooLarge(too_large));
    }

    #[test]
    fn slot_number_to_bytes_roundtrip() {
        let num = SlotNumber::new(123456789).unwrap();
        let bytes = num.to_bytes();
        let parsed = SlotNumber::try_from(bytes.as_slice()).unwrap();
        assert_eq!(parsed, num);
    }

    #[test]
    fn token_id_roundtrip() {
        let slot = SlotNumber::new(42).unwrap();
        let addr = H160::from_low_u64_be(0xdeadbeef);
        let token = TokenId::new(slot, addr);
        let bytes = token.to_bytes();
        let parsed = TokenId::try_from(bytes.as_slice()).unwrap();
        assert_eq!(parsed.slot_number(), slot);
        assert_eq!(parsed.h160_address(), addr);
    }

    #[test]
    fn slot_range_interval_requires_strict_order() {
        let slot = SlotNumber::new(100).unwrap();
        let err = SlotRange::interval(slot, slot).unwrap_err();
        assert_eq!(
            err,
            Brc721Error::InvalidSlotRange {
                start: 100,
                end: 100
            }
        );
    }

    #[test]
    fn register_ownership_roundtrip() {
        let collection_id = BitcoinCollectionId::new(100, 7);
        let slot_a = SlotNumber::new(1).unwrap();
        let slot_b = SlotNumber::new(10).unwrap();
        let slot_c = SlotNumber::new(25).unwrap();
        let range = SlotRange::interval(slot_b, slot_c).unwrap();
        let mapping_one = SlotMapping::new(1, vec![SlotRange::single(slot_a)]).unwrap();
        let mapping_two = SlotMapping::new(2, vec![range]).unwrap();
        let payload =
            RegisterOwnershipData::new(collection_id, vec![mapping_one, mapping_two]).unwrap();

        let bytes = payload.to_bytes();
        let parsed = RegisterOwnershipData::try_from(bytes.as_slice()).unwrap();
        assert_eq!(parsed.collection_id(), collection_id);
        assert_eq!(parsed.slot_mappings().len(), 2);
        assert_eq!(
            parsed.slot_mappings()[0].output_index(),
            payload.slot_mappings()[0].output_index()
        );
        assert_eq!(
            parsed.slot_mappings()[1].slot_ranges(),
            payload.slot_mappings()[1].slot_ranges()
        );
    }

    #[test]
    fn reject_empty_slot_mappings() {
        let collection_id = BitcoinCollectionId::new(1, 0);
        let err = RegisterOwnershipData::new(collection_id, vec![]).unwrap_err();
        assert_eq!(err, Brc721Error::MissingSlotMappings);
    }

    #[test]
    fn reject_empty_slot_range_list() {
        let err = SlotMapping::new(1, vec![]).unwrap_err();
        assert_eq!(err, Brc721Error::EmptySlotRangeList(1));
    }
}
