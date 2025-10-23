pub mod paths;
pub mod service;
pub mod types;

pub use service::{
    derive_next_address,
    get_core_balance,
    init_wallet,
    peek_address,
    setup_watchonly,
};
