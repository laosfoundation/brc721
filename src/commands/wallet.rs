use super::CommandRunner;
use crate::wallet::brc721_wallet::Brc721Wallet;
use crate::wallet::passphrase::prompt_passphrase;
use crate::{cli, context};
use age::secrecy::SecretString;
use anyhow::{anyhow, Context, Result};
use bdk_wallet::bip39::{Language, Mnemonic};
use rand::{rngs::OsRng, RngCore};

impl CommandRunner for cli::WalletCmd {
    fn run(&self, ctx: &context::Context) -> Result<()> {
        match self {
            cli::WalletCmd::Init {
                mnemonic,
                passphrase,
            } => run_init(ctx, mnemonic.clone(), passphrase.clone()),
            cli::WalletCmd::Generate => run_generate(),
            cli::WalletCmd::Address => run_address(ctx),
            cli::WalletCmd::Balance => run_balance(ctx),
            cli::WalletCmd::Rescan => run_rescan(ctx),
        }
    }
}

fn run_init(
    ctx: &context::Context,
    mnemonic: Option<String>,
    passphrase: Option<String>,
) -> Result<()> {
    // Check if wallet already exists
    if let Ok(wallet) = load_wallet(ctx) {
        wallet.setup_watch_only().context("setup watch only")?;
        log::info!("📡 Watch-only wallet '{}' ready in Core", wallet.id());
        return Ok(());
    }

    let mnemonic_str = mnemonic
        .as_ref()
        .ok_or_else(|| anyhow!("mnemonic is required when creating a new wallet"))?;

    let mnemonic =
        Mnemonic::parse_in(Language::English, mnemonic_str).context("invalid mnemonic")?;

    // Resolve passphrase
    let passphrase = resolve_passphrase_init(passphrase);

    // Create new wallet
    let wallet = Brc721Wallet::create(
        &ctx.data_dir,
        ctx.network,
        mnemonic,
        passphrase,
        &ctx.rpc_url,
        ctx.auth.clone(),
    )
    .context("wallet initialization")?;

    wallet.setup_watch_only().context("setup watch only")?;

    log::info!("🎉 New wallet created");
    log::info!("📡 Watch-only wallet '{}' ready in Core", wallet.id());
    Ok(())
}

fn run_address(ctx: &context::Context) -> Result<()> {
    let mut wallet = load_wallet(ctx)?;
    let addr = wallet
        .reveal_next_payment_address()
        .context("getting address")?;
    log::info!("🏠 {}", addr.address);
    Ok(())
}

fn run_balance(ctx: &context::Context) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    let balances = wallet.balances()?;
    log::info!("💰 {:?}", balances);
    Ok(())
}

fn run_rescan(ctx: &context::Context) -> Result<()> {
    let wallet = load_wallet(ctx)?;
    wallet
        .rescan_watch_only()
        .context("rescan watch-only wallet")?;
    log::info!("🔄 Rescan started for watch-only wallet '{}'", wallet.id());
    Ok(())
}

fn load_wallet(ctx: &context::Context) -> Result<Brc721Wallet> {
    Brc721Wallet::load(&ctx.data_dir, ctx.network, &ctx.rpc_url, ctx.auth.clone())
        .context("loading wallet")
}

fn resolve_passphrase_init(passphrase: Option<String>) -> SecretString {
    passphrase.map(SecretString::from).unwrap_or_else(|| {
        SecretString::from(prompt_passphrase().expect("prompt").unwrap_or_default())
    })
}

fn generate_mnemonic() -> Mnemonic {
    let mut entropy = [0u8; 16];
    OsRng.fill_bytes(&mut entropy);
    Mnemonic::from_entropy(&entropy).expect("mnemonic")
}

fn run_generate() -> Result<()> {
    let mnemonic = generate_mnemonic();
    println!("{}", mnemonic);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_mnemonic_is_valid_12_words() {
        let mnemonic = generate_mnemonic();
        assert_eq!(mnemonic.word_count(), 12);
        let parsed = Mnemonic::parse_in(Language::English, &mnemonic.to_string()).expect("parse");
        assert_eq!(parsed.word_count(), 12);
    }
}
