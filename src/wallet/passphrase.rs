use anyhow::{bail, Context, Result};
use std::io::IsTerminal;

/// Prompt the user for a new passphrase (double entry).
/// Returns Ok(None) if not interactive or if the user entered an empty passphrase.
pub fn prompt_passphrase() -> Result<Option<String>> {
    if std::io::stdin().is_terminal() {
        let p1 = rpassword::prompt_password("Enter passphrase: ").context("read passphrase")?;
        let p2 =
            rpassword::prompt_password("Confirm passphrase: ").context("confirm passphrase")?;
        if p1 != p2 {
            bail!("passphrases do not match");
        }
        if p1.is_empty() {
            Ok(None)
        } else {
            Ok(Some(p1))
        }
    } else {
        Ok(None)
    }
}
