//! Persistent Labcoat Network regtest faucet for Qubitcoin.

use crate::config::{get_qubitcoin_dir, IsomerConfig};
use bitcoin::absolute;
use bitcoin::consensus::encode::serialize_hex;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use bitcoin::sighash::SighashCache;
use bitcoin::transaction;
use bitcoin::{
    Address, Amount, CompressedPublicKey, EcdsaSighashType, Network, OutPoint, ScriptBuf, Sequence,
    Transaction, TxIn, TxOut, Txid, Witness,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

const FAUCET_STATE_SCHEMA: u32 = 1;
const COINBASE_SATS: u64 = 5_000_000_000;
const COINBASE_MATURITY: u64 = 100;
const FEE_RATE_SATS_VB: u64 = 2;
const DUST_SATS: u64 = 546;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FaucetUtxo {
    txid: String,
    vout: u32,
    amount_sats: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FaucetState {
    schema: u32,
    secret_key: String,
    utxos: Vec<FaucetUtxo>,
}

fn state_path() -> std::path::PathBuf {
    get_qubitcoin_dir().join("labcoat-faucet.json")
}

fn load_or_create() -> Result<FaucetState, String> {
    load_or_create_at(&state_path())
}

fn load_or_create_at(path: &std::path::Path) -> Result<FaucetState, String> {
    if let Ok(raw) = std::fs::read_to_string(path) {
        let state: FaucetState =
            serde_json::from_str(&raw).map_err(|e| format!("Invalid faucet state: {e}"))?;
        if state.schema != FAUCET_STATE_SCHEMA {
            return Err(format!("Unsupported faucet state schema {}", state.schema));
        }
        secret_key(&state)?;
        return Ok(state);
    }

    let mut bytes = [0_u8; 32];
    let secret = loop {
        rand::thread_rng().fill_bytes(&mut bytes);
        if let Ok(secret) = SecretKey::from_slice(&bytes) {
            break secret;
        }
    };
    let state = FaucetState {
        schema: FAUCET_STATE_SCHEMA,
        secret_key: hex::encode(secret.secret_bytes()),
        utxos: Vec::new(),
    };
    save_at(&state, path)?;
    Ok(state)
}

fn save(state: &FaucetState) -> Result<(), String> {
    save_at(state, &state_path())
}

fn save_at(state: &FaucetState, path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create faucet directory: {e}"))?;
    }
    let bytes =
        serde_json::to_vec_pretty(state).map_err(|e| format!("Failed to encode faucet: {e}"))?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes).map_err(|e| format!("Failed to write faucet state: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&temporary)
            .map_err(|e| format!("Failed to inspect faucet state: {e}"))?
            .permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(&temporary, permissions)
            .map_err(|e| format!("Failed to protect faucet state: {e}"))?;
    }
    std::fs::rename(temporary, path).map_err(|e| format!("Failed to persist faucet state: {e}"))
}

fn secret_key(state: &FaucetState) -> Result<SecretKey, String> {
    let bytes = hex::decode(&state.secret_key).map_err(|e| format!("Invalid faucet key: {e}"))?;
    SecretKey::from_slice(&bytes).map_err(|e| format!("Invalid faucet key: {e}"))
}

fn key_material(state: &FaucetState) -> Result<(SecretKey, PublicKey, Address), String> {
    let secret = secret_key(state)?;
    let secp = Secp256k1::new();
    let public = secret.public_key(&secp);
    let address = Address::p2wpkh(&CompressedPublicKey(public), Network::Regtest);
    Ok((secret, public, address))
}

pub fn address() -> Result<String, String> {
    let state = load_or_create()?;
    Ok(key_material(&state)?.2.to_string())
}

/// Wait for Qubitcoin, then create 101 faucet coinbase outputs when the
/// persisted faucet has no recorded funding.
pub fn bootstrap_blocking(config: &IsomerConfig) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if crate::rpc::call_blocking(config, "getblockcount", serde_json::json!([])).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }

    let mut state = load_or_create()?;
    if !state.utxos.is_empty() {
        return wait_for_indexers(config);
    }
    let address = key_material(&state)?.2.to_string();
    let hashes = crate::rpc::call_blocking(
        config,
        "generatetoaddress",
        serde_json::json!([101, address]),
    )?;
    let hashes = hashes
        .as_array()
        .ok_or_else(|| "Invalid generatetoaddress response".to_string())?;
    for hash in hashes {
        let hash = hash
            .as_str()
            .ok_or_else(|| "Invalid generated block hash".to_string())?;
        let block = crate::rpc::call_blocking(config, "getblock", serde_json::json!([hash, 1]))?;
        let txid = block
            .get("tx")
            .and_then(|txs| txs.as_array())
            .and_then(|txs| txs.first())
            .and_then(|txid| txid.as_str())
            .ok_or_else(|| "Generated block has no coinbase transaction".to_string())?;
        state.utxos.push(FaucetUtxo {
            txid: txid.to_string(),
            vout: 0,
            amount_sats: COINBASE_SATS,
        });
    }
    save(&state)?;
    wait_for_indexers(config)
}

fn wait_for_indexers(config: &IsomerConfig) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let height = crate::rpc::call_blocking(config, "getblockcount", serde_json::json!([]))
            .ok()
            .and_then(|value| value.as_u64());
        if let Some(height) = height {
            let synchronized = ["alkanes", "esplora"].into_iter().all(|label| {
                crate::rpc::call_blocking(config, "secondaryheight", serde_json::json!([label]))
                    .ok()
                    .and_then(|value| value.as_u64())
                    == Some(height)
            });
            if synchronized {
                return Ok(());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Err("Qubitcoin secondary indexers did not reach chain height within 30 seconds".into())
}

pub async fn fund(
    config: &IsomerConfig,
    recipient: &str,
    amount_btc: f64,
) -> Result<String, String> {
    if !amount_btc.is_finite() || amount_btc <= 0.0 {
        return Err("Funding amount must be a positive finite BTC value".into());
    }
    let amount = Amount::from_btc(amount_btc)
        .map_err(|e| format!("Invalid funding amount: {e}"))?
        .to_sat();
    let recipient_script = recipient_script(recipient)?;

    let mut state = load_or_create()?;
    let (secret, public, faucet_address) = key_material(&state)?;
    let faucet_script = faucet_address.script_pubkey();

    let mut available = Vec::new();
    for utxo in &state.utxos {
        let result = crate::rpc::call(
            config,
            "gettxout",
            serde_json::json!([utxo.txid, utxo.vout]),
            std::time::Duration::from_secs(5),
        )
        .await?;
        if result.is_null() {
            continue;
        }
        let confirmations = result
            .get("confirmations")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let coinbase = result
            .get("coinbase")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if is_spendable(confirmations, coinbase) {
            available.push(utxo.clone());
        }
    }

    let (selected, change) = select_for_amount(&available, amount)?;

    let inputs = selected
        .iter()
        .map(|utxo| {
            Ok(TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_str(&utxo.txid)
                        .map_err(|e| format!("Invalid faucet outpoint: {e}"))?,
                    vout: utxo.vout,
                },
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut tx_outputs = vec![TxOut {
        value: Amount::from_sat(amount),
        script_pubkey: recipient_script,
    }];
    if change >= DUST_SATS {
        tx_outputs.push(TxOut {
            value: Amount::from_sat(change),
            script_pubkey: faucet_script.clone(),
        });
    }
    let mut transaction = Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: inputs,
        output: tx_outputs,
    };
    sign_p2wpkh(
        &mut transaction,
        &selected,
        &faucet_script,
        &secret,
        &public,
    )?;
    let raw = serialize_hex(&transaction);
    let returned = crate::rpc::call(
        config,
        "sendrawtransaction",
        serde_json::json!([raw, 0.0]),
        std::time::Duration::from_secs(30),
    )
    .await?;
    let txid = returned
        .as_str()
        .ok_or_else(|| "Invalid sendrawtransaction response".to_string())?
        .to_string();
    Txid::from_str(&txid)
        .map_err(|e| format!("Qubitcoin returned an invalid transaction id: {e}"))?;
    state.utxos.retain(|utxo| {
        !selected
            .iter()
            .any(|spent| spent.txid == utxo.txid && spent.vout == utxo.vout)
    });
    if change >= DUST_SATS {
        state.utxos.push(FaucetUtxo {
            txid: txid.clone(),
            vout: 1,
            amount_sats: change,
        });
    }
    save(&state)?;
    Ok(txid)
}

fn recipient_script(recipient: &str) -> Result<ScriptBuf, String> {
    Address::from_str(recipient)
        .map_err(|e| format!("Invalid regtest address: {e}"))?
        .require_network(Network::Regtest)
        .map(|address| address.script_pubkey())
        .map_err(|e| format!("Invalid regtest address: {e}"))
}

fn estimated_fee(inputs: usize, outputs: usize) -> u64 {
    (10 + inputs as u64 * 68 + outputs as u64 * 31) * FEE_RATE_SATS_VB
}

fn is_spendable(confirmations: u64, coinbase: bool) -> bool {
    !coinbase || confirmations >= COINBASE_MATURITY
}

fn select_for_amount(
    available: &[FaucetUtxo],
    amount: u64,
) -> Result<(Vec<FaucetUtxo>, u64), String> {
    let mut selected = Vec::new();
    let mut total = 0_u64;
    for utxo in available {
        selected.push(utxo.clone());
        total = total.saturating_add(utxo.amount_sats);
        if total >= amount.saturating_add(estimated_fee(selected.len(), 1)) {
            break;
        }
    }
    if total < amount.saturating_add(estimated_fee(selected.len(), 1)) {
        return Err("Labcoat Network faucet has insufficient mature funds".into());
    }
    let two_output_fee = estimated_fee(selected.len(), 2);
    let change = if total >= amount.saturating_add(two_output_fee + DUST_SATS) {
        total - amount - two_output_fee
    } else {
        0
    };
    Ok((selected, change))
}

fn sign_p2wpkh(
    transaction: &mut Transaction,
    selected: &[FaucetUtxo],
    script_pubkey: &ScriptBuf,
    secret: &SecretKey,
    public: &PublicKey,
) -> Result<(), String> {
    let secp = Secp256k1::new();
    let mut cache = SighashCache::new(transaction);
    for (index, utxo) in selected.iter().enumerate() {
        let sighash_type = EcdsaSighashType::All;
        let sighash = cache
            .p2wpkh_signature_hash(
                index,
                script_pubkey,
                Amount::from_sat(utxo.amount_sats),
                sighash_type,
            )
            .map_err(|e| format!("Failed to sign faucet transaction: {e}"))?;
        let message = Message::from_digest(sighash.to_byte_array());
        let signature = bitcoin::ecdsa::Signature {
            signature: secp.sign_ecdsa(&message, secret),
            sighash_type,
        };
        *cache
            .witness_mut(index)
            .ok_or_else(|| "Missing faucet transaction input".to_string())? =
            Witness::p2wpkh(&signature, public);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_scales_with_inputs_and_outputs() {
        assert_eq!(estimated_fee(1, 2), 280);
        assert!(estimated_fee(2, 2) > estimated_fee(1, 2));
        assert!(estimated_fee(1, 2) > estimated_fee(1, 1));
    }

    #[test]
    fn coinbase_requires_maturity_but_change_does_not() {
        assert!(!is_spendable(99, true));
        assert!(is_spendable(100, true));
        assert!(is_spendable(0, false));
    }

    #[test]
    fn selection_preserves_exact_recipient_amount_and_calculates_change() {
        let utxo = FaucetUtxo {
            txid: Txid::all_zeros().to_string(),
            vout: 0,
            amount_sats: COINBASE_SATS,
        };
        let amount = 100_000_000;
        let (selected, change) = select_for_amount(&[utxo], amount).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(
            change,
            COINBASE_SATS - amount - estimated_fee(1, 2),
            "the recipient amount is not reduced to pay the fee"
        );
    }

    #[test]
    fn selection_reports_insufficient_funds() {
        let utxo = FaucetUtxo {
            txid: Txid::all_zeros().to_string(),
            vout: 0,
            amount_sats: 1_000,
        };
        assert_eq!(
            select_for_amount(&[utxo], 1_000).unwrap_err(),
            "Labcoat Network faucet has insufficient mature funds"
        );
    }

    #[test]
    fn rejects_invalid_and_non_regtest_addresses_before_rpc() {
        assert!(recipient_script("not-an-address").is_err());
        assert!(recipient_script("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh").is_err());
        assert!(recipient_script("bcrt1q9zuctyd46l7sdedccdk47335lzsmjz2wngdv3u").is_ok());
    }

    #[test]
    fn faucet_key_persists_across_reloads() {
        let unique = format!(
            "labcoat-faucet-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let directory = std::env::temp_dir().join(unique);
        let path = directory.join("faucet.json");
        let first = load_or_create_at(&path).unwrap();
        let second = load_or_create_at(&path).unwrap();
        assert_eq!(first.secret_key, second.secret_key);
        assert_eq!(address_from_state(&first), address_from_state(&second));
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn address_from_state(state: &FaucetState) -> String {
        key_material(state).unwrap().2.to_string()
    }

    #[test]
    fn signs_a_standard_p2wpkh_transaction() {
        let secret = SecretKey::from_slice(&[7_u8; 32]).unwrap();
        let secp = Secp256k1::new();
        let public = secret.public_key(&secp);
        let script =
            Address::p2wpkh(&CompressedPublicKey(public), Network::Regtest).script_pubkey();
        let utxo = FaucetUtxo {
            txid: Txid::all_zeros().to_string(),
            vout: 0,
            amount_sats: 50_000,
        };
        let mut tx = Transaction {
            version: transaction::Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(49_000),
                script_pubkey: script.clone(),
            }],
        };
        sign_p2wpkh(&mut tx, &[utxo], &script, &secret, &public).unwrap();
        assert_eq!(tx.input[0].witness.len(), 2);
    }
}
