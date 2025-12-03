use crate::types::Brc721Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterOwnershipData {
    pub collection_block_height: u64,
    pub collection_tx_index: u32,
    pub groups: Vec<OwnershipGroup>,
}

impl RegisterOwnershipData {
    pub const COLLECTION_BYTES: usize = std::mem::size_of::<u64>() + std::mem::size_of::<u32>();
    pub const GROUP_COUNT_BYTES: usize = 1;

    pub fn to_bytes(&self) -> Vec<u8> {
        assert!(
            self.groups.len() <= u8::MAX as usize,
            "group count exceeds u8::MAX"
        );
        for group in &self.groups {
            assert!(
                group.slot_ranges.len() <= u8::MAX as usize,
                "slot range count exceeds u8::MAX"
            );
            assert!(group.output_index > 0, "output indexes start at 1");
        }

        let mut out = Vec::new();
        out.extend_from_slice(&self.collection_block_height.to_be_bytes());
        out.extend_from_slice(&self.collection_tx_index.to_be_bytes());
        out.push(self.groups.len() as u8);
        for group in &self.groups {
            out.push(group.output_index);
            out.push(group.slot_ranges.len() as u8);
            for range in &group.slot_ranges {
                out.extend_from_slice(&range.start.to_be_bytes());
                out.extend_from_slice(&range.end.to_be_bytes());
            }
        }
        out
    }

    pub fn collection_key_parts(&self) -> (u64, u32) {
        (self.collection_block_height, self.collection_tx_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipGroup {
    pub output_index: u8,
    pub slot_ranges: Vec<SlotRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotRange {
    pub start: SlotNumber,
    pub end: SlotNumber,
}

impl SlotRange {
    pub const SERIALIZED_LEN: usize = SlotNumber::BYTE_LEN * 2;

    pub fn new(start: SlotNumber, end: SlotNumber) -> Result<Self, Brc721Error> {
        if start > end {
            return Err(Brc721Error::InvalidSlotRange {
                start: start.value(),
                end: end.value(),
            });
        }
        Ok(Self { start, end })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenId {
    pub slot: SlotNumber,
    pub initial_owner: [u8; 20],
}

impl TokenId {
    pub fn new(slot: SlotNumber, initial_owner: [u8; 20]) -> Self {
        Self {
            slot,
            initial_owner,
        }
    }

    pub fn initial_owner(&self) -> [u8; 20] {
        self.initial_owner
    }

    pub fn slot(&self) -> SlotNumber {
        self.slot
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotNumber(u128);

impl SlotNumber {
    pub const BYTE_LEN: usize = 12;
    pub const MAX: u128 = (1u128 << 96) - 1;

    pub fn new(value: u128) -> Result<Self, Brc721Error> {
        if value > Self::MAX {
            return Err(Brc721Error::InvalidSlotValue(value));
        }
        Ok(SlotNumber(value))
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, Brc721Error> {
        if bytes.len() != Self::BYTE_LEN {
            return Err(Brc721Error::InvalidOwnershipPayload(
                "slot number must be 12 bytes",
            ));
        }
        let mut buf = [0u8; 16];
        buf[4..].copy_from_slice(bytes);
        let value = u128::from_be_bytes(buf);
        SlotNumber::new(value)
    }

    pub fn to_be_bytes(self) -> [u8; Self::BYTE_LEN] {
        let be = self.0.to_be_bytes();
        let mut out = [0u8; Self::BYTE_LEN];
        out.copy_from_slice(&be[4..]);
        out
    }

    pub fn value(&self) -> u128 {
        self.0
    }
}

impl TryFrom<&[u8]> for RegisterOwnershipData {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() < Self::COLLECTION_BYTES + Self::GROUP_COUNT_BYTES {
            return Err(Brc721Error::InvalidOwnershipPayload(
                "ownership payload too short",
            ));
        }

        let (block_bytes, rest) = bytes.split_at(std::mem::size_of::<u64>());
        let (tx_bytes, rest) = rest.split_at(std::mem::size_of::<u32>());
        let block_height = u64::from_be_bytes(block_bytes.try_into().unwrap());
        let tx_index = u32::from_be_bytes(tx_bytes.try_into().unwrap());

        let (&group_count_byte, mut cursor) =
            rest.split_first()
                .ok_or(Brc721Error::InvalidOwnershipPayload(
                    "missing ownership groups",
                ))?;
        if group_count_byte == 0 {
            return Err(Brc721Error::InvalidOwnershipPayload(
                "at least one ownership group is required",
            ));
        }

        let mut groups = Vec::with_capacity(group_count_byte as usize);

        for _ in 0..group_count_byte {
            if cursor.len() < 2 {
                return Err(Brc721Error::InvalidOwnershipPayload(
                    "ownership group header is incomplete",
                ));
            }
            let output_index = cursor[0];
            let range_count = cursor[1];
            cursor = &cursor[2..];

            if output_index == 0 {
                return Err(Brc721Error::InvalidOwnershipPayload(
                    "output index 0 reserved for OP_RETURN",
                ));
            }
            if range_count == 0 {
                return Err(Brc721Error::InvalidOwnershipPayload(
                    "slot range count cannot be zero",
                ));
            }

            let mut ranges = Vec::with_capacity(range_count as usize);
            for _ in 0..range_count {
                if cursor.len() < SlotRange::SERIALIZED_LEN {
                    return Err(Brc721Error::InvalidOwnershipPayload(
                        "slot range payload incomplete",
                    ));
                }
                let (start_bytes, rem) = cursor.split_at(SlotNumber::BYTE_LEN);
                let (end_bytes, rem) = rem.split_at(SlotNumber::BYTE_LEN);
                cursor = rem;
                let start = SlotNumber::from_be_bytes(start_bytes)?;
                let end = SlotNumber::from_be_bytes(end_bytes)?;
                let range = SlotRange::new(start, end)?;
                ranges.push(range);
            }

            groups.push(OwnershipGroup {
                output_index,
                slot_ranges: ranges,
            });
        }

        if !cursor.is_empty() {
            return Err(Brc721Error::InvalidOwnershipPayload(
                "unexpected trailing data in payload",
            ));
        }

        Ok(RegisterOwnershipData {
            collection_block_height: block_height,
            collection_tx_index: tx_index,
            groups,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(value: u128) -> SlotNumber {
        SlotNumber::new(value).unwrap()
    }

    #[test]
    fn slot_number_roundtrip() {
        let value = (1u128 << 80) + 12345;
        let slot = SlotNumber::new(value).unwrap();
        let bytes = slot.to_be_bytes();
        let parsed = SlotNumber::from_be_bytes(&bytes).unwrap();
        assert_eq!(parsed, slot);
    }

    #[test]
    fn slot_number_rejects_large_values() {
        let value = SlotNumber::MAX + 1;
        match SlotNumber::new(value) {
            Err(Brc721Error::InvalidSlotValue(v)) => assert_eq!(v, value),
            other => panic!("expected InvalidSlotValue, got {:?}", other),
        }
    }

    #[test]
    fn ownership_data_roundtrip() {
        let data = RegisterOwnershipData {
            collection_block_height: 42,
            collection_tx_index: 7,
            groups: vec![
                OwnershipGroup {
                    output_index: 1,
                    slot_ranges: vec![
                        SlotRange::new(slot(0), slot(10)).unwrap(),
                        SlotRange::new(slot(20), slot(25)).unwrap(),
                    ],
                },
                OwnershipGroup {
                    output_index: 2,
                    slot_ranges: vec![SlotRange::new(slot(100), slot(100)).unwrap()],
                },
            ],
        };

        let bytes = data.to_bytes();
        let parsed = RegisterOwnershipData::try_from(bytes.as_slice()).unwrap();
        assert_eq!(parsed, data);
    }

    #[test]
    fn ownership_payload_rejects_zero_groups() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&42u64.to_be_bytes());
        bytes.extend_from_slice(&7u32.to_be_bytes());
        bytes.push(0); // zero groups

        let err = RegisterOwnershipData::try_from(bytes.as_slice()).unwrap_err();
        match err {
            Brc721Error::InvalidOwnershipPayload(msg) => {
                assert!(msg.contains("at least one ownership group"))
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn ownership_payload_rejects_output_zero() {
        let slot = slot(1);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u64.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.push(1); // one group
        bytes.push(0); // invalid output index
        bytes.push(1); // one range
        bytes.extend_from_slice(&slot.to_be_bytes());
        bytes.extend_from_slice(&slot.to_be_bytes());

        let err = RegisterOwnershipData::try_from(bytes.as_slice()).unwrap_err();
        match err {
            Brc721Error::InvalidOwnershipPayload(msg) => {
                assert!(msg.contains("output index 0"))
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

}
