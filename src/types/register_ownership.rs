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
        slots: SlotRanges,
    ) -> Result<Self, Brc721Error> {
        Self::new(
            collection_height,
            collection_tx_index,
            vec![OwnershipGroup {
                ranges: slots.into_ranges(),
            }],
        )
    }

    pub fn validate_in_tx(&self, bitcoin_tx: &Transaction) -> Result<(), Brc721Error> {
        let output_count = bitcoin_tx.output.len();
        for (group_index, _group) in self.groups.iter().enumerate() {
            let output_index = group_index + 1;
            if output_index >= output_count {
                return Err(Brc721Error::TxError(format!(
                    "register-ownership output_index {} out of bounds (tx outputs={})",
                    output_index, output_count
                )));
            }
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.validate()
            .expect("register ownership payload must be valid before serialization");

        use crate::types::varint96::VarInt96;

        let group_count = self.groups.len() as u128;
        let group_count_varint =
            VarInt96::new(group_count).expect("validated group count must fit 96-bit varint");

        let height = VarInt96::new(self.collection_height as u128)
            .expect("u64 always fits in 96-bit varint");
        let tx_index = VarInt96::new(self.collection_tx_index as u128)
            .expect("u32 always fits in 96-bit varint");

        let mut out =
            Vec::with_capacity(height.size() + tx_index.size() + group_count_varint.size());
        height.encode_into(&mut out);
        tx_index.encode_into(&mut out);
        group_count_varint.encode_into(&mut out);

        for group in &self.groups {
            let range_count: u8 = group
                .ranges
                .len()
                .try_into()
                .expect("range count must fit in u8");
            out.push(range_count);

            for range in &group.ranges {
                if range.start == range.end {
                    out.push(0x00); // single slot
                    VarInt96::new(range.start)
                        .expect("validated slot must fit 96 bits")
                        .encode_into(&mut out);
                } else {
                    out.push(0x01); // slot range
                    VarInt96::new(range.start)
                        .expect("validated slot must fit 96 bits")
                        .encode_into(&mut out);
                    VarInt96::new(range.end)
                        .expect("validated slot must fit 96 bits")
                        .encode_into(&mut out);
                }
            }
        }

        out
    }

    fn validate(&self) -> Result<(), Brc721Error> {
        if self.groups.is_empty() {
            return Err(Brc721Error::InvalidGroupCount(0));
        }

        for group in &self.groups {
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

        let collection_height_raw = take_varint96(bytes, &mut cursor)?;
        let collection_height: u64 = collection_height_raw.try_into().map_err(|_| {
            Brc721Error::TxError(format!(
                "collection height {} out of range (max {})",
                collection_height_raw,
                u64::MAX
            ))
        })?;

        let collection_tx_index_raw = take_varint96(bytes, &mut cursor)?;
        let collection_tx_index: u32 = collection_tx_index_raw.try_into().map_err(|_| {
            Brc721Error::TxError(format!(
                "collection tx_index {} out of range (max {})",
                collection_tx_index_raw,
                u32::MAX
            ))
        })?;

        let group_count_raw = take_varint96(bytes, &mut cursor)?;
        let group_count: usize = group_count_raw.try_into().map_err(|_| {
            Brc721Error::TxError(format!(
                "group_count {} out of range (max {})",
                group_count_raw,
                usize::MAX
            ))
        })?;
        if group_count == 0 {
            return Err(Brc721Error::InvalidGroupCount(0));
        }

        let mut groups = Vec::new();

        for _ in 0..group_count {
            let range_count = take_bytes(bytes, &mut cursor, 1)?[0];
            if range_count == 0 {
                return Err(Brc721Error::InvalidRangeCount(range_count));
            }

            let mut ranges = Vec::with_capacity(range_count as usize);
            for _ in 0..range_count {
                let tag = take_bytes(bytes, &mut cursor, 1)?[0];
                match tag {
                    0x00 => {
                        let slot = take_varint96(bytes, &mut cursor)?;
                        ranges.push(SlotRange {
                            start: slot,
                            end: slot,
                        });
                    }
                    0x01 => {
                        let start = take_varint96(bytes, &mut cursor)?;
                        let end = take_varint96(bytes, &mut cursor)?;
                        if start > end {
                            return Err(Brc721Error::InvalidSlotRange(start, end));
                        }
                        if start == end {
                            return Err(Brc721Error::TxError(format!(
                                "invalid slot range: start {start} must be strictly less than end {end} (use a single slot item instead)"
                            )));
                        }
                        ranges.push(SlotRange { start, end });
                    }
                    other => {
                        return Err(Brc721Error::TxError(format!(
                            "unknown slot item tag: {other}"
                        )));
                    }
                }
            }

            groups.push(OwnershipGroup { ranges });
        }

        if cursor != bytes.len() {
            return Err(Brc721Error::InvalidLength(cursor, bytes.len()));
        }

        Ok(Self {
            collection_height,
            collection_tx_index,
            groups,
        })
    }
}

fn take_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], Brc721Error> {
    if bytes.len() < *cursor + len {
        return Err(Brc721Error::InvalidLength(*cursor + len, bytes.len()));
    }
    let slice = &bytes[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice)
}

fn take_varint96(bytes: &[u8], cursor: &mut usize) -> Result<u128, Brc721Error> {
    use crate::types::varint96::VarInt96;

    let slice = bytes
        .get(*cursor..)
        .ok_or_else(|| Brc721Error::TxError("varint96: cursor out of bounds".to_string()))?;
    let (value, consumed) =
        VarInt96::decode(slice).map_err(|e| Brc721Error::TxError(e.to_string()))?;
    *cursor += consumed;
    Ok(value.value())
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
        let data = RegisterOwnershipData::for_single_output(0, 0, slots)
            .expect("valid minimal register ownership payload");
        let bytes = data.to_bytes();
        let parsed = RegisterOwnershipData::try_from(bytes.as_slice()).expect("parse succeeds");
        assert_eq!(parsed, data);
    }

    #[test]
    fn validate_in_tx_rejects_out_of_bounds_output_index() {
        use bitcoin::{absolute, transaction, Amount, ScriptBuf, TxOut};

        let slots = SlotRanges::from_str("0").expect("slots parse");
        let data = RegisterOwnershipData::for_single_output(0, 0, slots)
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
        let bytes = vec![0, 0, 0]; // height=0 (varint), tx_index=0 (varint), group_count=0 (varint)
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
            1, // group count (varint)
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
        use crate::types::varint96::VarInt96;

        let start = 10u128;
        let end = 5u128;

        let mut bytes = vec![
            1, // height varint
            2, // tx index varint
            1, // group count (varint)
            1, // range count
        ];

        bytes.push(0x01); // slot range tag
        VarInt96::new(start)
            .expect("start fits")
            .encode_into(&mut bytes);
        VarInt96::new(end)
            .expect("end fits")
            .encode_into(&mut bytes);

        let res = RegisterOwnershipData::try_from(bytes.as_slice());
        match res {
            Err(Brc721Error::InvalidSlotRange(s, e)) => {
                assert_eq!(s, start);
                assert_eq!(e, end);
            }
            other => panic!("expected InvalidSlotRange, got {:?}", other),
        }
    }

    #[test]
    fn rejects_equal_slot_range_endpoints_in_tagged_range_item() {
        use crate::types::varint96::VarInt96;

        let start = 42u128;
        let end = 42u128;

        let mut bytes = vec![
            1, // height varint
            2, // tx index varint
            1, // group count (varint)
            1, // range count
        ];

        bytes.push(0x01); // slot range tag
        VarInt96::new(start)
            .expect("start fits")
            .encode_into(&mut bytes);
        VarInt96::new(end)
            .expect("end fits")
            .encode_into(&mut bytes);

        let res = RegisterOwnershipData::try_from(bytes.as_slice());
        match res {
            Err(Brc721Error::TxError(msg)) => {
                assert!(msg.contains("strictly less"));
                assert!(msg.contains("single slot"));
            }
            other => panic!("expected TxError, got {:?}", other),
        }
    }
}
