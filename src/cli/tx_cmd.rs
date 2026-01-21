use clap::Subcommand;
use ethereum_types::H160;

use crate::storage::traits::CollectionKey;
use crate::types::SlotRanges;

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
        about = "Register BRC-721 collection ownership",
        long_about = "Create and broadcast a transaction that registers BRC-721 collection ownership for a given collection id (HEIGHT:TX_INDEX)."
    )]
    RegisterOwnership {
        #[arg(
            long = "collection-id",
            value_name = "HEIGHT:TX_INDEX",
            help = "Collection id in the form <block_height>:<tx_index> (e.g. 850123:0)",
            required = true
        )]
        collection_id: CollectionKey,
        #[arg(
            long = "slots",
            value_name = "RANGES",
            help = "Comma-separated slot ranges (inclusive) and/or single slots, e.g. '0..=9,10..=19' or '42' (ranges require start < end)",
            required = true
        )]
        slots: SlotRanges,
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
    #[command(
        about = "Send BRC-721 assets to an address (implicit transfer)",
        long_about = "Create and broadcast a transaction that spends one or more BRC-721 ownership UTXOs (without an OP_RETURN payload), triggering the protocol's implicit transfer rules to move the assets to the target address."
    )]
    SendAssets {
        #[arg(value_name = "ADDRESS", help = "Target address to receive the assets")]
        to: String,
        #[arg(
            long = "outpoint",
            value_name = "TXID:VOUT",
            required = true,
            num_args = 1..,
            help = "Ownership outpoint(s) to spend (repeat --outpoint for multiple)"
        )]
        outpoints: Vec<String>,
        #[arg(
            long = "dust-sat",
            value_name = "SATOSHI",
            default_value_t = 546,
            help = "Satoshis to attach to each asset-carrying output"
        )]
        dust_sat: u64,
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
        about = "Mix BRC-721 assets across outputs (explicit mapping)",
        long_about = "Create and broadcast a mix transaction that maps NFT indices across the provided ownership inputs to specific outputs using an OP_RETURN payload. Exactly one output mapping must be marked as the complement set. Use a single output with :complement to merge all tokens into one output."
    )]
    Mix {
        #[arg(
            long = "outpoint",
            value_name = "TXID:VOUT",
            required = true,
            num_args = 1..,
            help = "Ownership outpoint(s) to spend (repeat --outpoint for multiple)"
        )]
        outpoints: Vec<String>,
        #[arg(
            long = "output",
            value_name = "ADDRESS:RANGES|ADDRESS:complement",
            required = true,
            num_args = 1..,
            help = "Output mapping in the form ADDRESS:0..=10,12 (end inclusive) or ADDRESS:complement (repeat --output to define outputs in order; a single complement output merges all tokens)"
        )]
        outputs: Vec<String>,
        #[arg(
            long = "dust-sat",
            value_name = "SATOSHI",
            default_value_t = 546,
            help = "Satoshis to attach to each asset-carrying output"
        )]
        dust_sat: u64,
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
