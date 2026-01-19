mod brc721_parser;
mod mix;
mod register_collection;
mod register_ownership;
mod traits;

pub use brc721_parser::Brc721Parser;
pub use traits::BlockParser;

use crate::storage::traits::{OwnershipRange, OwnershipUtxo};

#[derive(Debug)]
pub(crate) struct TokenInput {
    pub prev_txid: String,
    pub prev_vout: u32,
    pub groups: Vec<(OwnershipUtxo, Vec<OwnershipRange>)>,
}
