use ethereum_types::H160;
use std::{fmt, num::ParseIntError, str::FromStr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionKey {
    pub block_height: u64,
    pub tx_index: u32,
}

impl CollectionKey {
    pub fn new(block_height: u64, tx_index: u32) -> Self {
        Self {
            block_height,
            tx_index,
        }
    }
}

impl fmt::Display for CollectionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.block_height, self.tx_index)
    }
}

#[derive(Debug)]
pub enum CollectionKeyParseError {
    InvalidFormat,
    InvalidBlock(ParseIntError),
    InvalidTx(ParseIntError),
    ExtraData,
}

impl fmt::Display for CollectionKeyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CollectionKeyParseError::InvalidFormat => write!(f, "invalid collection key format"),
            CollectionKeyParseError::InvalidBlock(err) => {
                write!(f, "invalid block height in collection key: {err}")
            }
            CollectionKeyParseError::InvalidTx(err) => {
                write!(f, "invalid tx index in collection key: {err}")
            }
            CollectionKeyParseError::ExtraData => write!(f, "collection key has extra data"),
        }
    }
}

impl std::error::Error for CollectionKeyParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CollectionKeyParseError::InvalidBlock(err) => Some(err),
            CollectionKeyParseError::InvalidTx(err) => Some(err),
            _ => None,
        }
    }
}

impl FromStr for CollectionKey {
    type Err = CollectionKeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(':');
        let block_str = parts.next().ok_or(CollectionKeyParseError::InvalidFormat)?;
        let tx_str = parts.next().ok_or(CollectionKeyParseError::InvalidFormat)?;
        if parts.next().is_some() {
            return Err(CollectionKeyParseError::ExtraData);
        }
        let block_height = block_str
            .parse::<u64>()
            .map_err(CollectionKeyParseError::InvalidBlock)?;
        let tx_index = tx_str
            .parse::<u32>()
            .map_err(CollectionKeyParseError::InvalidTx)?;
        Ok(CollectionKey::new(block_height, tx_index))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection {
    pub key: CollectionKey,
    pub evm_collection_address: H160,
    pub rebaseable: bool,
}
