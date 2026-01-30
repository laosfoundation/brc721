use crate::storage::traits::StorageRead;
use anyhow::{anyhow, Context, Result};
use bitcoin::{Address, OutPoint};
use bitcoincore_rpc::json::ListUnspentResultEntry;
use std::collections::BTreeSet;

pub fn ownership_outpoints_for_wallet<S: StorageRead>(
    storage: &S,
    wallet_utxos: &[ListUnspentResultEntry],
) -> Result<BTreeSet<OutPoint>> {
    let mut wallet_token_outpoints = BTreeSet::new();
    for utxo in wallet_utxos {
        let txid = utxo.txid.to_string();
        let vout = utxo.vout;
        if storage
            .list_unspent_ownership_utxos_by_outpoint(&txid, vout)
            .with_context(|| format!("query ownership ranges for {txid}:{vout}"))?
            .is_empty()
        {
            continue;
        }

        wallet_token_outpoints.insert(OutPoint {
            txid: utxo.txid,
            vout,
        });
    }

    Ok(wallet_token_outpoints)
}

pub fn lock_outpoints_for_fees(
    ownership_outpoints: &BTreeSet<OutPoint>,
    spending: &[OutPoint],
) -> Vec<OutPoint> {
    let spending_set = spending.iter().cloned().collect::<BTreeSet<_>>();
    ownership_outpoints
        .difference(&spending_set)
        .cloned()
        .collect()
}

pub fn select_non_ownership_utxo_for_address(
    wallet_utxos: &[ListUnspentResultEntry],
    owner_address: &Address,
    disallowed: &BTreeSet<OutPoint>,
) -> Result<OutPoint> {
    let script_pubkey = owner_address.script_pubkey();
    let mut best: Option<&ListUnspentResultEntry> = None;

    for utxo in wallet_utxos {
        if utxo.script_pub_key != script_pubkey {
            continue;
        }
        let outpoint = OutPoint {
            txid: utxo.txid,
            vout: utxo.vout,
        };
        if disallowed.contains(&outpoint) {
            continue;
        }
        match best {
            None => best = Some(utxo),
            Some(current) => {
                if utxo.amount.to_sat() > current.amount.to_sat() {
                    best = Some(utxo);
                }
            }
        }
    }

    let Some(utxo) = best else {
        return Err(anyhow!(
            "no spendable UTXO found for init-owner address {} (fund it with a non-NFT UTXO)",
            owner_address
        ));
    };

    Ok(OutPoint {
        txid: utxo.txid,
        vout: utxo.vout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{Amount, Network, Txid};
    use std::str::FromStr;

    fn make_utxo(
        txid_hex: &str,
        vout: u32,
        script_pub_key: bitcoin::ScriptBuf,
        amount_sat: u64,
    ) -> ListUnspentResultEntry {
        ListUnspentResultEntry {
            txid: Txid::from_str(txid_hex).unwrap(),
            vout,
            address: None,
            label: None,
            redeem_script: None,
            witness_script: None,
            script_pub_key,
            amount: Amount::from_sat(amount_sat),
            confirmations: 1,
            spendable: true,
            solvable: true,
            descriptor: None,
            safe: true,
        }
    }

    #[test]
    fn select_non_ownership_utxo_for_address_skips_disallowed() {
        let address = Address::from_str("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh")
            .unwrap()
            .require_network(Network::Bitcoin)
            .unwrap();
        let script_pub_key = address.script_pubkey();
        let utxo_nft = make_utxo(&"00".repeat(32), 0, script_pub_key.clone(), 1000);
        let utxo_ok = make_utxo(&"11".repeat(32), 1, script_pub_key.clone(), 2000);

        let mut disallowed = BTreeSet::new();
        disallowed.insert(OutPoint {
            txid: utxo_nft.txid,
            vout: utxo_nft.vout,
        });

        let selected =
            select_non_ownership_utxo_for_address(&[utxo_nft, utxo_ok], &address, &disallowed)
                .unwrap();
        assert_eq!(selected.txid, Txid::from_str(&"11".repeat(32)).unwrap());
        assert_eq!(selected.vout, 1);
    }

    #[test]
    fn select_non_ownership_utxo_for_address_errors_when_only_disallowed() {
        let address = Address::from_str("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh")
            .unwrap()
            .require_network(Network::Bitcoin)
            .unwrap();
        let script_pub_key = address.script_pubkey();
        let utxo_nft = make_utxo(&"00".repeat(32), 0, script_pub_key.clone(), 1000);

        let mut disallowed = BTreeSet::new();
        disallowed.insert(OutPoint {
            txid: utxo_nft.txid,
            vout: utxo_nft.vout,
        });

        let res = select_non_ownership_utxo_for_address(&[utxo_nft], &address, &disallowed);
        assert!(res.is_err());
    }
}
