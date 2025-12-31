use crate::types::{Brc721Error, Brc721Token};
use bitcoin::Transaction;
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRange {
    pub start: u128,
    pub end: u128,
}

#[derive(Debug)]
pub struct SlotRangesParseError {
    message: String,
}

impl SlotRangesParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SlotRangesParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SlotRangesParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRanges(Vec<SlotRange>);

impl SlotRanges {
    pub(crate) fn into_ranges(self) -> Vec<SlotRange> {
        self.0
    }
}

impl FromStr for SlotRanges {
    type Err = SlotRangesParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw = s.trim();
        if raw.is_empty() {
            return Err(SlotRangesParseError::new(
                "slots cannot be empty (expected e.g. '0..=9,10..=19')",
            ));
        }

        let mut ranges = Vec::new();
        for part in raw.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(SlotRangesParseError::new(
                    "slots contains an empty range (expected e.g. '0..=9,10..=19')",
                ));
            }

            match part.split_once("..=") {
                Some((start, end)) => {
                    let start = parse_slot_str(start)?;
                    let end = parse_slot_str(end)?;

                    if start > end {
                        return Err(SlotRangesParseError::new(format!(
                            "invalid slot range '{part}': start {start} is greater than end {end}"
                        )));
                    }

                    if start == end {
                        return Err(SlotRangesParseError::new(format!(
                            "invalid slot range '{part}': start {start} must be strictly less than end {end} (use '{start}' for a single slot)"
                        )));
                    }

                    ranges.push(SlotRange { start, end });
                }
                None => {
                    let single = parse_slot_str(part)?;
                    ranges.push(SlotRange {
                        start: single,
                        end: single,
                    });
                }
            };
        }

        if ranges.len() > u8::MAX as usize {
            return Err(SlotRangesParseError::new(format!(
                "too many slot ranges (got {}, max {})",
                ranges.len(),
                u8::MAX
            )));
        }

        // Disallow overlapping slots across ranges.
        if ranges.len() > 1 {
            let mut sorted = ranges.clone();
            sorted.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));

            let mut last = &sorted[0];
            for current in sorted.iter().skip(1) {
                if current.start <= last.end {
                    return Err(SlotRangesParseError::new(format!(
                        "overlapping slot ranges are not allowed: {}..={} overlaps {}..={}",
                        last.start, last.end, current.start, current.end
                    )));
                }
                last = current;
            }
        }

        Ok(Self(ranges))
    }
}

fn parse_slot_str(s: &str) -> Result<u128, SlotRangesParseError> {
    let raw = s.trim();
    if raw.is_empty() {
        return Err(SlotRangesParseError::new("slot number cannot be empty"));
    }
    let slot: u128 = raw
        .parse()
        .map_err(|_| SlotRangesParseError::new(format!("invalid slot number '{raw}'")))?;
    if slot > Brc721Token::MAX_SLOT {
        return Err(SlotRangesParseError::new(format!(
            "slot number {slot} exceeds max {}",
            Brc721Token::MAX_SLOT
        )));
    }
    Ok(slot)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipGroup {
    pub output_index: u8,
    pub ranges: Vec<SlotRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterOwnershipData {
    pub collection_height: u64,
    pub collection_tx_index: u32,
    pub groups: Vec<OwnershipGroup>,
}

impl RegisterOwnershipData {
    pub fn new(
        collection_height: u64,
        collection_tx_index: u32,
        groups: Vec<OwnershipGroup>,
    ) -> Result<Self, Brc721Error> {
        let data = Self {
            collection_height,
            collection_tx_index,
            groups,
        };
        data.validate()?;
        Ok(data)
    }

    pub fn for_single_output(
        collection_height: u64,
        collection_tx_index: u32,
        output_index: u8,
        slots: SlotRanges,
    ) -> Result<Self, Brc721Error> {
        Self::new(
            collection_height,
            collection_tx_index,
            vec![OwnershipGroup {
                output_index,
                ranges: slots.into_ranges(),
            }],
        )
    }

    pub fn validate_in_tx(&self, bitcoin_tx: &Transaction) -> Result<(), Brc721Error> {
        let output_count = bitcoin_tx.output.len();
        for group in &self.groups {
            if group.output_index as usize >= output_count {
                return Err(Brc721Error::TxError(format!(
                    "register-ownership output_index {} out of bounds (tx outputs={})",
                    group.output_index, output_count
                )));
            }
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.validate()
            .expect("register ownership payload must be valid before serialization");

        use bitcoin::consensus::encode::{Encodable, VarInt};

        let group_count: u8 = self
            .groups
            .len()
            .try_into()
            .expect("group count must fit in u8");

        let height_vi = VarInt::from(self.collection_height);
        let tx_index_vi = VarInt::from(self.collection_tx_index);

        let mut out = Vec::with_capacity(height_vi.size() + tx_index_vi.size() + 1);
        height_vi
            .consensus_encode(&mut out)
            .expect("Vec<u8> writes are infallible");
        tx_index_vi
            .consensus_encode(&mut out)
            .expect("Vec<u8> writes are infallible");
        out.push(group_count);

        for group in &self.groups {
            let range_count: u8 = group
                .ranges
                .len()
                .try_into()
                .expect("range count must fit in u8");
            out.push(group.output_index);
            out.push(range_count);

            for range in &group.ranges {
                let start = range.start.to_be_bytes();
                let end = range.end.to_be_bytes();
                let start_bytes = &start[start.len() - Brc721Token::SLOT_BYTES..];
                let end_bytes = &end[end.len() - Brc721Token::SLOT_BYTES..];
                out.extend_from_slice(start_bytes);
                out.extend_from_slice(end_bytes);
            }
        }

        out
    }

    fn validate(&self) -> Result<(), Brc721Error> {
        let group_count = self
            .groups
            .len()
            .try_into()
            .map_err(|_| Brc721Error::InvalidGroupCount(u8::MAX))?;

        if group_count == 0 {
            return Err(Brc721Error::InvalidGroupCount(group_count));
        }

        for group in &self.groups {
            if group.output_index == 0 {
                return Err(Brc721Error::InvalidOutputIndex(group.output_index));
            }

            let range_count = group
                .ranges
                .len()
                .try_into()
                .map_err(|_| Brc721Error::InvalidRangeCount(u8::MAX))?;

            if range_count == 0 {
                return Err(Brc721Error::InvalidRangeCount(range_count));
            }

            for range in &group.ranges {
                if range.start > range.end {
                    return Err(Brc721Error::InvalidSlotRange(range.start, range.end));
                }
                if range.start > Brc721Token::MAX_SLOT {
                    return Err(Brc721Error::InvalidSlotNumber(range.start));
                }
                if range.end > Brc721Token::MAX_SLOT {
                    return Err(Brc721Error::InvalidSlotNumber(range.end));
                }
            }
        }

        Ok(())
    }
}

impl TryFrom<&[u8]> for RegisterOwnershipData {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let mut cursor = 0usize;
        let total_len = bytes.len();

        let mut take = |len: usize| -> Result<&[u8], Brc721Error> {
            if total_len < cursor + len {
                return Err(Brc721Error::InvalidLength(cursor + len, total_len));
            }
            let slice = &bytes[cursor..cursor + len];
            cursor += len;
            Ok(slice)
        };

        let collection_height = take_varint(&mut take)?;
        let collection_tx_index_raw = take_varint(&mut take)?;
        let collection_tx_index: u32 = collection_tx_index_raw.try_into().map_err(|_| {
            Brc721Error::TxError(format!(
                "collection tx_index {} out of range (max {})",
                collection_tx_index_raw,
                u32::MAX
            ))
        })?;

        let group_count_bytes = take(1)?;
        let group_count = group_count_bytes[0];
        if group_count == 0 {
            return Err(Brc721Error::InvalidGroupCount(group_count));
        }

        let mut groups = Vec::with_capacity(group_count as usize);

        for _ in 0..group_count {
            let output_index = take(1)?[0];
            if output_index == 0 {
                return Err(Brc721Error::InvalidOutputIndex(output_index));
            }

            let range_count = take(1)?[0];
            if range_count == 0 {
                return Err(Brc721Error::InvalidRangeCount(range_count));
            }

            let mut ranges = Vec::with_capacity(range_count as usize);
            for _ in 0..range_count {
                let start_bytes = take(Brc721Token::SLOT_BYTES)?;
                let end_bytes = take(Brc721Token::SLOT_BYTES)?;
                let start = parse_slot(start_bytes)?;
                let end = parse_slot(end_bytes)?;
                if start > end {
                    return Err(Brc721Error::InvalidSlotRange(start, end));
                }
                ranges.push(SlotRange { start, end });
            }

            groups.push(OwnershipGroup {
                output_index,
                ranges,
            });
        }

        if cursor != total_len {
            return Err(Brc721Error::InvalidLength(cursor, total_len));
        }

        Ok(Self {
            collection_height,
            collection_tx_index,
            groups,
        })
    }
}

fn take_varint<'a>(
    take: &mut impl FnMut(usize) -> Result<&'a [u8], Brc721Error>,
) -> Result<u64, Brc721Error> {
    let prefix = take(1)?[0];
    match prefix {
        n @ 0x00..=0xFC => Ok(n as u64),
        0xFD => {
            let slice: [u8; 2] = take(2)?.try_into().expect("slice length checked");
            let value = u16::from_le_bytes(slice) as u64;
            if value < 0xFD {
                return Err(Brc721Error::TxError(
                    bitcoin::consensus::encode::Error::NonMinimalVarInt.to_string(),
                ));
            }
            Ok(value)
        }
        0xFE => {
            let slice: [u8; 4] = take(4)?.try_into().expect("slice length checked");
            let value = u32::from_le_bytes(slice) as u64;
            if value < 0x1_0000 {
                return Err(Brc721Error::TxError(
                    bitcoin::consensus::encode::Error::NonMinimalVarInt.to_string(),
                ));
            }
            Ok(value)
        }
        0xFF => {
            let slice: [u8; 8] = take(8)?.try_into().expect("slice length checked");
            let value = u64::from_le_bytes(slice);
            if value < 0x1_0000_0000 {
                return Err(Brc721Error::TxError(
                    bitcoin::consensus::encode::Error::NonMinimalVarInt.to_string(),
                ));
            }
            Ok(value)
        }
    }
}

fn parse_slot(bytes: &[u8]) -> Result<u128, Brc721Error> {
    debug_assert_eq!(bytes.len(), Brc721Token::SLOT_BYTES);

    let mut padded = [0u8; 16];
    let offset = padded.len() - Brc721Token::SLOT_BYTES;
    padded[offset..].copy_from_slice(bytes);
    let slot = u128::from_be_bytes(padded);

    if slot > Brc721Token::MAX_SLOT {
        return Err(Brc721Error::InvalidSlotNumber(slot));
    }

    Ok(slot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_ranges_parse_allows_single_and_multiple_ranges() {
        let slots = SlotRanges::from_str("0..=9, 42,10..=19").expect("parse");
        assert_eq!(
            slots,
            SlotRanges(vec![
                SlotRange { start: 0, end: 9 },
                SlotRange { start: 42, end: 42 },
                SlotRange { start: 10, end: 19 },
            ])
        );
    }

    #[test]
    fn slot_ranges_parse_rejects_start_greater_than_end() {
        let err = SlotRanges::from_str("9..=0").unwrap_err();
        assert!(err.to_string().contains("start 9 is greater than end 0"));
    }

    #[test]
    fn slot_ranges_parse_rejects_equal_range_endpoints() {
        let err = SlotRanges::from_str("42..=42").unwrap_err();
        assert!(err.to_string().contains("must be strictly less than"));
        assert!(err.to_string().contains("use '42'"));
    }

    #[test]
    fn slot_ranges_parse_rejects_overlapping_ranges() {
        let err = SlotRanges::from_str("0..=5,3..=7").unwrap_err();
        assert!(err.to_string().contains("overlapping slot ranges"));
    }

    fn sample_payload() -> RegisterOwnershipData {
        RegisterOwnershipData::new(
            840_000,
            2,
            vec![OwnershipGroup {
                output_index: 1,
                ranges: vec![SlotRange { start: 0, end: 9 }],
            }],
        )
        .expect("valid sample payload")
    }

    #[test]
    fn roundtrip_register_ownership_data() {
        let data = sample_payload();
        let bytes = data.to_bytes();
        let parsed = RegisterOwnershipData::try_from(bytes.as_slice()).expect("parse succeeds");
        assert_eq!(parsed, data);
    }

    #[test]
    fn minimal_payload_is_valid_and_roundtrips() {
        let slots = SlotRanges::from_str("0").expect("slots parse");
        let data = RegisterOwnershipData::for_single_output(0, 0, 1, slots)
            .expect("valid minimal register ownership payload");
        let bytes = data.to_bytes();
        let parsed = RegisterOwnershipData::try_from(bytes.as_slice()).expect("parse succeeds");
        assert_eq!(parsed, data);
    }

    #[test]
    fn validate_in_tx_rejects_out_of_bounds_output_index() {
        use bitcoin::{absolute, transaction, Amount, ScriptBuf, TxOut};

        let slots = SlotRanges::from_str("0").expect("slots parse");
        let data = RegisterOwnershipData::for_single_output(0, 0, 1, slots)
            .expect("valid minimal register ownership payload");
        let tx = bitcoin::Transaction {
            version: transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![],
            output: vec![TxOut {
                value: Amount::from_sat(0),
                script_pubkey: ScriptBuf::new(),
            }],
        };

        match data.validate_in_tx(&tx) {
            Err(Brc721Error::TxError(_)) => {}
            other => panic!("expected TxError, got {:?}", other),
        }
    }

    #[test]
    fn rejects_zero_group_count() {
        let bytes = vec![0, 0, 0]; // height=0 (varint), tx_index=0 (varint), group_count=0
        let res = RegisterOwnershipData::try_from(bytes.as_slice());
        match res {
            Err(Brc721Error::InvalidGroupCount(0)) => {}
            other => panic!("expected InvalidGroupCount(0), got {:?}", other),
        }
    }

    #[test]
    fn rejects_zero_range_count() {
        // Build bytes manually: header + one group with range_count = 0
        let bytes = vec![
            1, // height varint
            2, // tx index varint
            1, // group count
            1, // output index
            0, // range count (invalid)
        ];

        let res = RegisterOwnershipData::try_from(bytes.as_slice());
        match res {
            Err(Brc721Error::InvalidRangeCount(0)) => {}
            other => panic!("expected InvalidRangeCount(0), got {:?}", other),
        }
    }

    #[test]
    fn rejects_inverted_slot_range() {
        let start = 10u128;
        let end = 5u128;

        let mut bytes = vec![
            1, // height varint
            2, // tx index varint
            1, // group count
            1, // output index
            1, // range count
        ];

        let start_bytes = start.to_be_bytes();
        let end_bytes = end.to_be_bytes();
        bytes.extend_from_slice(&start_bytes[start_bytes.len() - Brc721Token::SLOT_BYTES..]);
        bytes.extend_from_slice(&end_bytes[end_bytes.len() - Brc721Token::SLOT_BYTES..]);

        let res = RegisterOwnershipData::try_from(bytes.as_slice());
        match res {
            Err(Brc721Error::InvalidSlotRange(s, e)) => {
                assert_eq!(s, start);
                assert_eq!(e, end);
            }
            other => panic!("expected InvalidSlotRange, got {:?}", other),
        }
    }
}
