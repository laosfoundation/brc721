use crate::types::varint96::VarInt96;
use crate::types::Brc721Error;
use bitcoin::Transaction;
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRange {
    pub start: u128,
    pub end: u128,
}

#[derive(Debug)]
pub struct IndexRangesParseError {
    message: String,
}

impl IndexRangesParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for IndexRangesParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for IndexRangesParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRanges(Vec<IndexRange>);

impl IndexRanges {
    pub(crate) fn into_ranges(self) -> Vec<IndexRange> {
        self.0
    }
}

impl FromStr for IndexRanges {
    type Err = IndexRangesParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw = s.trim();
        if raw.is_empty() {
            return Err(IndexRangesParseError::new(
                "index ranges cannot be empty (expected e.g. '0..=9,10,20..=25')",
            ));
        }

        let mut ranges = Vec::new();
        for part in raw.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(IndexRangesParseError::new(
                    "index ranges contains an empty range (expected e.g. '0..=9,10,20..=25')",
                ));
            }

            if let Some((start, end)) = part.split_once("..=") {
                let start = parse_index_str(start)?;
                let end = parse_index_str(end)?;

                if start > end {
                    return Err(IndexRangesParseError::new(format!(
                        "invalid index range '{part}': start {start} must be less than or equal to end {end}"
                    )));
                }

                if end == VarInt96::MAX_VALUE {
                    let max_inclusive = VarInt96::MAX_VALUE - 1;
                    return Err(IndexRangesParseError::new(format!(
                        "invalid index range '{part}': inclusive end {end} is too large (max {max_inclusive})"
                    )));
                }

                ranges.push(IndexRange {
                    start,
                    end: end + 1,
                });
            } else if let Some((start, end)) = part.split_once("..") {
                let start = parse_index_str(start)?;
                let end = parse_index_str(end)?;

                if start >= end {
                    return Err(IndexRangesParseError::new(format!(
                        "invalid index range '{part}': start {start} must be less than end {end}"
                    )));
                }

                ranges.push(IndexRange { start, end });
            } else {
                let single = parse_index_str(part)?;
                if single == VarInt96::MAX_VALUE {
                    return Err(IndexRangesParseError::new(format!(
                        "single index {single} is too large to express as a range"
                    )));
                }
                ranges.push(IndexRange {
                    start: single,
                    end: single + 1,
                });
            }
        }

        if ranges.len() > 1 {
            let mut sorted = ranges.clone();
            sorted.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));

            let mut last = &sorted[0];
            for current in sorted.iter().skip(1) {
                if current.start < last.end {
                    return Err(IndexRangesParseError::new(format!(
                        "overlapping index ranges are not allowed: {}..{} overlaps {}..{}",
                        last.start, last.end, current.start, current.end
                    )));
                }
                last = current;
            }
        }

        Ok(Self(ranges))
    }
}

fn parse_index_str(s: &str) -> Result<u128, IndexRangesParseError> {
    let raw = s.trim();
    if raw.is_empty() {
        return Err(IndexRangesParseError::new("index cannot be empty"));
    }
    let index: u128 = raw
        .parse()
        .map_err(|_| IndexRangesParseError::new(format!("invalid index '{raw}'")))?;
    if index > VarInt96::MAX_VALUE {
        return Err(IndexRangesParseError::new(format!(
            "index {index} exceeds max {}",
            VarInt96::MAX_VALUE
        )));
    }
    Ok(index)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixData {
    pub output_ranges: Vec<Vec<IndexRange>>,
    pub complement_index: usize,
}

impl MixData {
    pub fn new(
        output_ranges: Vec<Vec<IndexRange>>,
        complement_index: usize,
    ) -> Result<Self, Brc721Error> {
        let data = Self {
            output_ranges,
            complement_index,
        };
        data.validate_basic()?;
        Ok(data)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.validate_basic()
            .expect("mix payload must be valid before serialization");

        let output_count = VarInt96::new(self.output_ranges.len() as u128)
            .expect("validated output count must fit 96-bit varint");
        let mut out = Vec::with_capacity(output_count.size());
        output_count.encode_into(&mut out);

        for (index, ranges) in self.output_ranges.iter().enumerate() {
            let range_count = if index == self.complement_index {
                0u128
            } else {
                ranges.len() as u128
            };
            let range_count_varint =
                VarInt96::new(range_count).expect("validated range count must fit 96-bit varint");
            range_count_varint.encode_into(&mut out);

            if index == self.complement_index {
                continue;
            }

            for range in ranges {
                VarInt96::new(range.start)
                    .expect("validated range start must fit 96-bit varint")
                    .encode_into(&mut out);
                VarInt96::new(range.end)
                    .expect("validated range end must fit 96-bit varint")
                    .encode_into(&mut out);
            }
        }

        out
    }

    pub fn validate_in_tx(&self, bitcoin_tx: &Transaction) -> Result<(), Brc721Error> {
        self.validate_basic()?;

        let output_count = bitcoin_tx.output.len();
        if output_count < self.output_ranges.len() + 1 {
            return Err(Brc721Error::TxError(format!(
                "mix output count {} out of bounds (tx outputs={})",
                self.output_ranges.len(),
                output_count
            )));
        }

        Ok(())
    }

    pub fn max_explicit_end(&self) -> Option<u128> {
        self.output_ranges
            .iter()
            .enumerate()
            .filter(|(idx, _)| *idx != self.complement_index)
            .flat_map(|(_, ranges)| ranges.iter())
            .map(|range| range.end)
            .max()
    }

    pub fn validate_token_count(&self, total_tokens: u128) -> Result<(), Brc721Error> {
        let max_end = self.max_explicit_end().ok_or_else(|| {
            Brc721Error::TxError("mix requires at least one explicit range".into())
        })?;

        if max_end == 0 {
            return Err(Brc721Error::TxError(
                "mix requires at least one non-empty explicit range".into(),
            ));
        }

        if total_tokens < max_end {
            return Err(Brc721Error::TxError(format!(
                "mix index range out of bounds (token_count={}, max_index={})",
                total_tokens, max_end
            )));
        }

        Ok(())
    }

    fn validate_basic(&self) -> Result<(), Brc721Error> {
        if self.output_ranges.len() < 2 {
            return Err(Brc721Error::TxError(
                "mix requires at least 2 output mappings".into(),
            ));
        }

        if self.complement_index >= self.output_ranges.len() {
            return Err(Brc721Error::TxError(
                "mix complement index out of bounds".into(),
            ));
        }

        for (index, ranges) in self.output_ranges.iter().enumerate() {
            if index == self.complement_index {
                if !ranges.is_empty() {
                    return Err(Brc721Error::TxError(
                        "mix complement output must not define explicit ranges".into(),
                    ));
                }
                continue;
            }

            if ranges.is_empty() {
                return Err(Brc721Error::TxError(
                    "mix output ranges cannot be empty (use complement output instead)".into(),
                ));
            }

            for range in ranges {
                if range.start >= range.end {
                    return Err(Brc721Error::TxError(format!(
                        "invalid mix index range {}..{} (start must be less than end)",
                        range.start, range.end
                    )));
                }
                if range.start > VarInt96::MAX_VALUE || range.end > VarInt96::MAX_VALUE {
                    return Err(Brc721Error::TxError(format!(
                        "mix index range {}..{} exceeds max {}",
                        range.start,
                        range.end,
                        VarInt96::MAX_VALUE
                    )));
                }
            }
        }

        let mut all_ranges = Vec::new();
        for (output_index, ranges) in self.output_ranges.iter().enumerate() {
            if output_index == self.complement_index {
                continue;
            }
            for range in ranges {
                all_ranges.push((range.start, range.end));
            }
        }

        if all_ranges.is_empty() {
            return Err(Brc721Error::TxError(
                "mix requires at least one explicit range".into(),
            ));
        }

        all_ranges.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut last = all_ranges[0];
        for current in all_ranges.into_iter().skip(1) {
            if current.0 < last.1 {
                return Err(Brc721Error::TxError(format!(
                    "overlapping mix index ranges are not allowed: {}..{} overlaps {}..{}",
                    last.0, last.1, current.0, current.1
                )));
            }
            last = current;
        }

        Ok(())
    }
}

impl TryFrom<&[u8]> for MixData {
    type Error = Brc721Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let mut cursor = 0usize;

        let output_count_raw = take_varint96(bytes, &mut cursor)?;
        let output_count: usize = output_count_raw.try_into().map_err(|_| {
            Brc721Error::TxError(format!(
                "mix output_count {} out of range (max {})",
                output_count_raw,
                usize::MAX
            ))
        })?;
        if output_count < 2 {
            return Err(Brc721Error::TxError(format!(
                "mix output_count must be at least 2, got {}",
                output_count
            )));
        }

        let mut output_ranges = Vec::with_capacity(output_count);
        let mut complement_index: Option<usize> = None;

        for index in 0..output_count {
            let range_count_raw = take_varint96(bytes, &mut cursor)?;
            let range_count: usize = range_count_raw.try_into().map_err(|_| {
                Brc721Error::TxError(format!(
                    "mix range_count {} out of range (max {})",
                    range_count_raw,
                    usize::MAX
                ))
            })?;

            if range_count == 0 {
                if complement_index.is_some() {
                    return Err(Brc721Error::TxError(
                        "mix cannot define multiple complement outputs".into(),
                    ));
                }
                complement_index = Some(index);
                output_ranges.push(Vec::new());
                continue;
            }

            let mut ranges = Vec::with_capacity(range_count);
            for _ in 0..range_count {
                let start = take_varint96(bytes, &mut cursor)?;
                let end = take_varint96(bytes, &mut cursor)?;
                if start >= end {
                    return Err(Brc721Error::TxError(format!(
                        "invalid mix index range {}..{} (start must be less than end)",
                        start, end
                    )));
                }
                ranges.push(IndexRange { start, end });
            }

            output_ranges.push(ranges);
        }

        if cursor != bytes.len() {
            return Err(Brc721Error::InvalidLength(cursor, bytes.len()));
        }

        let complement_index = complement_index.ok_or_else(|| {
            Brc721Error::TxError("mix requires exactly one complement output".into())
        })?;

        MixData::new(output_ranges, complement_index)
    }
}

fn take_varint96(bytes: &[u8], cursor: &mut usize) -> Result<u128, Brc721Error> {
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
    use std::str::FromStr;

    #[test]
    fn index_ranges_parse_allows_single_and_ranges() {
        let ranges = IndexRanges::from_str("0..=2, 5,7..10").expect("parse");
        assert_eq!(
            ranges,
            IndexRanges(vec![
                IndexRange { start: 0, end: 3 },
                IndexRange { start: 5, end: 6 },
                IndexRange { start: 7, end: 10 },
            ])
        );
    }

    #[test]
    fn index_ranges_parse_rejects_overlap() {
        let err = IndexRanges::from_str("0..=2,2..=3").unwrap_err();
        assert!(err.to_string().contains("overlapping index ranges"));
    }

    #[test]
    fn mix_roundtrip_ok() {
        let data = MixData::new(
            vec![
                vec![IndexRange { start: 0, end: 2 }],
                Vec::new(),
                vec![IndexRange { start: 2, end: 4 }],
            ],
            1,
        )
        .expect("valid mix data");

        let bytes = data.to_bytes();
        let parsed = MixData::try_from(bytes.as_slice()).expect("parse ok");
        assert_eq!(parsed, data);
    }

    #[test]
    fn mix_rejects_multiple_complements() {
        let bytes = vec![
            2, // output_count
            0, // output 0 (complement)
            0, // output 1 (complement) -> invalid
        ];
        let res = MixData::try_from(bytes.as_slice());
        assert!(res.is_err());
    }
}
