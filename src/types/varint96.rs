use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarInt96Error {
    EmptyInput,
    Unterminated,
    TooLong,
    NonMinimal,
    Overflow(u128),
}

impl fmt::Display for VarInt96Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VarInt96Error::EmptyInput => write!(f, "varint96: empty input"),
            VarInt96Error::Unterminated => write!(f, "varint96: unterminated value"),
            VarInt96Error::TooLong => write!(f, "varint96: value exceeds max length"),
            VarInt96Error::NonMinimal => write!(f, "varint96: non-minimal encoding"),
            VarInt96Error::Overflow(value) => {
                write!(f, "varint96: value {value} exceeds 96-bit maximum")
            }
        }
    }
}

impl std::error::Error for VarInt96Error {}

/// A minimally-encoded variable-length unsigned integer restricted to 96 bits.
///
/// Encoding is unsigned LEB128 (7 data bits per byte, little-endian), up to 14 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarInt96(u128);

impl VarInt96 {
    pub const MAX_VALUE: u128 = (1u128 << 96) - 1;
    pub const MAX_LEN: usize = 14;

    pub fn new(value: u128) -> Result<Self, VarInt96Error> {
        if value > Self::MAX_VALUE {
            return Err(VarInt96Error::Overflow(value));
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u128 {
        self.0
    }

    pub fn size(&self) -> usize {
        let mut value = self.0;
        let mut len = 1usize;
        while value >= 0x80 {
            value >>= 7;
            len += 1;
        }
        len
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        let mut value = self.0;
        loop {
            let byte = (value & 0x7F) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.size());
        self.encode_into(&mut out);
        out
    }

    /// Decode a `VarInt96` from the start of `bytes`, returning the value and bytes consumed.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize), VarInt96Error> {
        if bytes.is_empty() {
            return Err(VarInt96Error::EmptyInput);
        }

        let mut value: u128 = 0;
        let mut shift: u32 = 0;

        for index in 0..Self::MAX_LEN {
            let byte = *bytes.get(index).ok_or(VarInt96Error::Unterminated)?;
            let data = (byte & 0x7F) as u128;
            value |= data << shift;

            if (byte & 0x80) == 0 {
                let consumed = index + 1;
                if consumed > 1 && data == 0 {
                    return Err(VarInt96Error::NonMinimal);
                }
                if value > Self::MAX_VALUE {
                    return Err(VarInt96Error::Overflow(value));
                }
                return Ok((Self(value), consumed));
            }

            shift += 7;
        }

        Err(VarInt96Error::TooLong)
    }
}

impl TryFrom<u128> for VarInt96 {
    type Error = VarInt96Error;

    fn try_from(value: u128) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<VarInt96> for u128 {
    fn from(value: VarInt96) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encoding_decoding() {
        let values = [
            0u128,
            1,
            2,
            42,
            127,
            128,
            255,
            256,
            16_383,
            16_384,
            840_000,
            VarInt96::MAX_VALUE,
        ];

        for value in values {
            let vi = VarInt96::new(value).expect("value fits 96 bits");
            let encoded = vi.encode();
            assert_eq!(encoded.len(), vi.size());

            let (decoded, consumed) = VarInt96::decode(&encoded).expect("decode");
            assert_eq!(consumed, encoded.len());
            assert_eq!(decoded.value(), value);
        }
    }

    #[test]
    fn decode_rejects_non_minimal_encodings() {
        let err = VarInt96::decode(&[0x80, 0x00]).unwrap_err();
        assert_eq!(err, VarInt96Error::NonMinimal);
    }

    #[test]
    fn decode_rejects_overflow() {
        // 2^96 encoded as LEB128 => 13 continuation bytes + last byte with bit 5 set.
        let mut bytes = vec![0x80u8; 13];
        bytes.push(0x20);

        let err = VarInt96::decode(&bytes).unwrap_err();
        assert!(matches!(err, VarInt96Error::Overflow(_)));
    }

    #[test]
    fn decode_rejects_values_longer_than_max_len() {
        let mut bytes = vec![0x80u8; VarInt96::MAX_LEN];
        bytes.push(0x00);
        let err = VarInt96::decode(&bytes).unwrap_err();
        assert_eq!(err, VarInt96Error::TooLong);
    }

    #[test]
    fn new_rejects_overflow() {
        let err = VarInt96::new(1u128 << 96).unwrap_err();
        assert!(matches!(err, VarInt96Error::Overflow(_)));
    }
}

