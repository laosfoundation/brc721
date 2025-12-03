use crate::storage::traits::CollectionKey;
use crate::types::{SlotNumber, SlotRange};
use clap::Subcommand;
use ethereum_types::H160;
use std::str::FromStr;

#[derive(Subcommand, Debug, Clone)]
pub enum TxCmd {
    #[command(
        about = "Register a BRC-721 collection",
        long_about = "Create and broadcast a transaction that registers a BRC-721 collection, linking a 20-byte EVM (H160) address. Optionally mark the collection as rebaseable and set a custom fee rate (sat/vB)."
    )]
    RegisterCollection {
        #[arg(
            long = "evm-collection-address",
            value_name = "H160",
            help = "20-byte EVM collection address (H160)",
            required = true
        )]
        evm_collection_address: H160,
        #[arg(
            long,
            default_value_t = false,
            help = "Whether the collection is rebaseable"
        )]
        rebaseable: bool,
        #[arg(
            long = "fee-rate",
            value_name = "SAT/VB",
            required = false,
            help = "Fee rate in sat/vB (optional)"
        )]
        fee_rate: Option<f64>,
        #[arg(
            long,
            value_name = "PASSPHRASE",
            help = "Passphrase for signing",
            required = false
        )]
        passphrase: Option<String>,
    },
    #[command(
        about = "Register ownership for pre-minted tokens",
        long_about = "Create and broadcast a token ownership registration transaction that links slot ranges to new Bitcoin outputs for a previously registered collection."
    )]
    RegisterOwnership {
        #[arg(
            long = "collection-id",
            value_name = "BLOCK:TX",
            help = "Collection identifier in the form <block_height>:<tx_index>",
            required = true
        )]
        collection_id: CollectionKey,
        #[arg(
            long = "assignment",
            value_name = "ADDRESS[@SAT]:SLOTS",
            help = "Ownership assignment in the form address[@amount_sat]:slot-spec (e.g. bc1...@600:0-10,12,20-21). Repeat for multiple outputs.",
            required = true
        )]
        assignments: Vec<OwnershipAssignmentArg>,
        #[arg(
            long = "fee-rate",
            value_name = "SAT/VB",
            required = false,
            help = "Fee rate in sat/vB (optional)"
        )]
        fee_rate: Option<f64>,
        #[arg(
            long,
            value_name = "PASSPHRASE",
            help = "Passphrase for signing",
            required = false
        )]
        passphrase: Option<String>,
    },
    #[command(
        about = "Send a specific amount to an address",
        long_about = "Build and broadcast a transaction that sends the specified amount to the provided target address. Optionally set a custom fee rate (sat/vB)."
    )]
    SendAmount {
        #[arg(value_name = "ADDRESS", help = "Target address to receive the funds")]
        to: String,
        #[arg(
            long = "amount-sat",
            value_name = "SATOSHI",
            required = true,
            help = "Amount to send in satoshi"
        )]
        amount_sat: u64,
        #[arg(
            long = "fee-rate",
            value_name = "SAT/VB",
            required = false,
            help = "Fee rate in sat/vB (optional)"
        )]
        fee_rate: Option<f64>,
        #[arg(
            long,
            value_name = "PASSPHRASE",
            help = "Passphrase for signing",
            required = false
        )]
        passphrase: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct OwnershipAssignmentArg {
    pub address: String,
    pub amount_sat: u64,
    pub slot_ranges: Vec<SlotRange>,
}

impl FromStr for OwnershipAssignmentArg {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (address_amount, slots_part) = value
            .split_once(':')
            .ok_or_else(|| "assignment must be in ADDRESS[@SAT]:SLOTS format".to_string())?;

        if slots_part.trim().is_empty() {
            return Err("slot specification cannot be empty".to_string());
        }

        let (address_str, amount_sat) = parse_address_and_amount(address_amount)?;
        let slot_ranges = parse_slot_ranges(slots_part)?;
        if slot_ranges.is_empty() {
            return Err("at least one slot range must be provided".to_string());
        }
        if slot_ranges.len() > u8::MAX as usize {
            return Err("slot range count per assignment must be <= 255".to_string());
        }

        Ok(Self {
            address: address_str.to_string(),
            amount_sat,
            slot_ranges,
        })
    }
}

const DEFAULT_ASSIGNMENT_AMOUNT_SAT: u64 = 546;

fn parse_address_and_amount(input: &str) -> Result<(String, u64), String> {
    let (address_part, amount_sat) = if let Some((addr, amt)) = input.split_once('@') {
        let amount = amt
            .trim()
            .parse::<u64>()
            .map_err(|e| format!("invalid amount '{amt}': {e}"))?;
        (addr.trim(), amount)
    } else {
        (input.trim(), DEFAULT_ASSIGNMENT_AMOUNT_SAT)
    };

    if address_part.is_empty() {
        return Err("address cannot be empty".to_string());
    }
    if amount_sat == 0 {
        return Err("amount must be greater than 0 sat".to_string());
    }

    Ok((address_part.to_string(), amount_sat))
}

fn parse_slot_ranges(spec: &str) -> Result<Vec<SlotRange>, String> {
    spec.split(',')
        .map(|token| token.trim())
        .filter(|token| !token.is_empty())
        .map(parse_slot_range)
        .collect()
}

fn parse_slot_range(token: &str) -> Result<SlotRange, String> {
    let (start_str, end_str) = if let Some((start, end)) = token.split_once('-') {
        (start.trim(), end.trim())
    } else {
        (token.trim(), token.trim())
    };
    if start_str.is_empty() || end_str.is_empty() {
        return Err(format!("invalid slot range '{token}'"));
    }
    let start = parse_slot_value(start_str)?;
    let end = parse_slot_value(end_str)?;
    SlotRange::new(start, end).map_err(|e| format!("{e}"))
}

fn parse_slot_value(value: &str) -> Result<SlotNumber, String> {
    let parsed = value
        .parse::<u128>()
        .map_err(|e| format!("invalid slot value '{value}': {e}"))?;
    SlotNumber::new(parsed).map_err(|e| format!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_parses_address_amount_and_slots() {
        let arg: OwnershipAssignmentArg = "bc1qexample@700:0-10,15,20-25"
            .parse()
            .expect("parse assignment");
        assert_eq!(arg.address, "bc1qexample");
        assert_eq!(arg.amount_sat, 700);
        assert_eq!(arg.slot_ranges.len(), 3);
    }

    #[test]
    fn assignment_defaults_amount_when_missing() {
        let arg: OwnershipAssignmentArg = "bc1qlow:5".parse().expect("parse assignment");
        assert_eq!(arg.amount_sat, DEFAULT_ASSIGNMENT_AMOUNT_SAT);
        assert_eq!(arg.slot_ranges.len(), 1);
    }

    #[test]
    fn assignment_rejects_empty_slots() {
        let err = "bc1qfoo@600:"
            .parse::<OwnershipAssignmentArg>()
            .unwrap_err();
        assert!(err.contains("slot specification"));
    }

    #[test]
    fn assignment_rejects_invalid_slot_value() {
        let err = "bc1qfoo:abc".parse::<OwnershipAssignmentArg>().unwrap_err();
        assert!(err.contains("invalid slot value"));
    }
}
