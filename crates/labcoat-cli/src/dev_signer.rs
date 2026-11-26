//! The dev signer: Labcoat Network signs PSBTs for its built-in wallet.
//!
//! A page or a headless test builds an unsigned PSBT, posts it to the
//! network's JSON-RPC endpoint, and gets it back signed, with no browser
//! extension. That is only acceptable because of which key signs: the
//! built-in wallet's, a random key under a public passphrase that holds
//! regtest coins and nothing else. Everything here exists to make sure no
//! other key can end up behind the endpoint:
//!
//! - [`DevSigner::open`] refuses every network but Labcoat Network and
//!   every keystore but the built-in one, and the `labcoat node` process
//!   calls it with constants, never with `--network`, `--wallet-file`, or a
//!   `LABCOAT_*` setting;
//! - the keystore is unlocked with the built-in wallet's fixed passphrase
//!   only, so a keystore protected by a passphrase of the user's does not
//!   open;
//! - a symbolic link at the built-in path is refused, so the path cannot be
//!   pointed at a keystore kept somewhere else.
//!
//! A taproot key signs the same way on every network, so "regtest" is not a
//! property of a signature. The key is what keeps this to worthless coins.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use bitcoin::psbt::Psbt;
use bitcoin::Witness;
use labcoat_core::signer::{decode_psbt, encode_psbt, KeystoreSigner, Signer};
use labcoat_core::{LabcoatError, NetworkTarget};
use labcoat_runtime::node::RpcError;
use serde_json::{json, Value};

use crate::local_wallet;

/// Bitcoin Core's `RPC_WALLET_ERROR`: the wallet could not be opened or
/// could not sign.
const WALLET_ERROR: i64 = -4;
/// Bitcoin Core's `RPC_WALLET_NOT_FOUND`: the built-in wallet does not
/// exist yet.
const WALLET_NOT_FOUND: i64 = -18;
/// Bitcoin Core's `RPC_DESERIALIZATION_ERROR`.
const DECODE_FAILED: i64 = -22;

/// What identifies one version of the keystore file.
type Stamp = (Option<SystemTime>, u64);

pub struct DevSigner {
    wallet_file: PathBuf,
    /// The unlocked wallet, kept while the keystore file is unchanged:
    /// unlocking runs the keystore's key derivation, which is slow on
    /// purpose. `labcoat up` may create the wallet after the network starts.
    unlocked: Mutex<Option<(Stamp, Arc<KeystoreSigner>)>>,
}

impl DevSigner {
    /// The dev signer for `network`'s wallet at `wallet_file`, which must
    /// be Labcoat Network's built-in wallet.
    pub fn open(network: NetworkTarget, wallet_file: &Path) -> Result<Self, LabcoatError> {
        if network != NetworkTarget::Labcoat {
            return Err(LabcoatError::new(
                "SIGNER_UNSUPPORTED",
                format!(
                    "the dev signer exists only on Labcoat Network, not on {}",
                    network.id()
                ),
                "sign with a wallet of your own on other networks",
            ));
        }
        if !local_wallet::is_built_in(wallet_file) {
            return Err(LabcoatError::new(
                "SIGNER_UNSUPPORTED",
                format!(
                    "the dev signer only signs with the built-in Labcoat Network wallet, not {}",
                    wallet_file.display()
                ),
                "sign with `labcoat wallet sign-psbt` for a keystore of your own",
            ));
        }
        Ok(Self::at(wallet_file))
    }

    /// The dev signer of the Labcoat Network this process serves. No flag
    /// or environment setting chooses the network or the keystore.
    pub fn for_labcoat_network() -> Self {
        Self::open(
            NetworkTarget::Labcoat,
            &local_wallet::default_path(NetworkTarget::Labcoat),
        )
        .expect("the built-in wallet on Labcoat Network is what the dev signer signs with")
    }

    fn at(wallet_file: &Path) -> Self {
        Self {
            wallet_file: wallet_file.to_owned(),
            unlocked: Mutex::new(None),
        }
    }

    fn wallet(&self) -> Result<Arc<KeystoreSigner>, RpcError> {
        let metadata = std::fs::symlink_metadata(&self.wallet_file).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RpcError::new(
                    WALLET_NOT_FOUND,
                    "the built-in wallet does not exist yet; run `labcoat up`",
                )
            } else {
                RpcError::new(
                    WALLET_ERROR,
                    format!("cannot read the built-in wallet: {error}"),
                )
            }
        })?;
        if !metadata.is_file() {
            return Err(RpcError::new(
                WALLET_ERROR,
                format!(
                    "{} is not a regular file; the dev signer does not follow links to other keystores",
                    self.wallet_file.display()
                ),
            ));
        }
        let stamp = (metadata.modified().ok(), metadata.len());
        let mut unlocked = self.unlocked.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((known, wallet)) = unlocked.as_ref() {
            if *known == stamp {
                return Ok(wallet.clone());
            }
        }
        let wallet = Arc::new(
            KeystoreSigner::from_keystore_file(
                &self.wallet_file,
                local_wallet::PASSPHRASE,
                bitcoin::Network::Regtest,
            )
            .map_err(|error| {
                RpcError::new(
                    WALLET_ERROR,
                    format!(
                        "the built-in wallet cannot be opened with its fixed passphrase: {}",
                        error.message
                    ),
                )
            })?,
        );
        *unlocked = Some((stamp, wallet.clone()));
        Ok(wallet)
    }
}

fn wallet_error(error: LabcoatError) -> RpcError {
    RpcError::new(WALLET_ERROR, error.message)
}

/// Run a signer call to completion. The keystore signer never waits on
/// anything; the trait is async for signers that do.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> Result<T, RpcError> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|error| RpcError::new(WALLET_ERROR, error.to_string()))?
        .block_on(future))
}

fn is_final(input: &bitcoin::psbt::Input) -> bool {
    input.final_script_witness.is_some() || input.final_script_sig.is_some()
}

impl labcoat_runtime::node::DevSigner for DevSigner {
    fn addresses(&self) -> Result<Value, RpcError> {
        let wallet = self.wallet()?;
        let addresses: Vec<Value> = wallet
            .taproot_keys(1)
            .map_err(wallet_error)?
            .into_iter()
            .map(|key| {
                json!({
                    "address": key.address,
                    "type": "p2tr",
                    "derivationPath": key.derivation_path,
                    "internalKey": key.internal_key,
                    "outputKey": key.output_key,
                    "scriptPubKey": key.script_pubkey,
                })
            })
            .collect();
        Ok(json!({
            "network": "regtest",
            "fingerprint": wallet.fingerprint().map(|fingerprint| fingerprint.to_string()),
            "addresses": addresses,
        }))
    }

    fn sign_psbt(&self, raw: &str, finalize: bool) -> Result<Value, RpcError> {
        let mut psbt: Psbt = decode_psbt(raw.trim()).map_err(|error| {
            RpcError::new(
                DECODE_FAILED,
                format!("PSBT decode failed: {}", error.message),
            )
        })?;
        if let Some(index) = psbt
            .inputs
            .iter()
            .position(|input| input.witness_utxo.is_none())
        {
            return Err(RpcError::params(format!(
                "PSBT input {index} has no witness UTXO; a taproot signature commits to the output every input spends"
            )));
        }
        let wallet = self.wallet()?;

        let before = psbt.inputs.clone();
        block_on(wallet.sign_psbt(&mut psbt))?.map_err(wallet_error)?;
        let mut signed = Vec::new();
        for (index, was) in before.iter().enumerate() {
            if is_final(was) {
                // Nothing is added to an input someone already finalized.
                psbt.inputs[index] = was.clone();
            } else if was.tap_key_sig.is_none() && psbt.inputs[index].tap_key_sig.is_some() {
                signed.push(index);
            }
        }
        for &index in &signed {
            let input = &mut psbt.inputs[index];
            if let Some(requested) = input.sighash_type.filter(|kind| kind.to_u32() != 0) {
                return Err(RpcError::params(format!(
                    "PSBT input {index} asks for sighash type {requested}; the dev signer signs SIGHASH_DEFAULT only"
                )));
            }
            if finalize {
                let signature = input.tap_key_sig.take().expect("signed above");
                // BIP 174: a finalized input keeps its UTXO and the final
                // witness, and nothing that was only needed to sign.
                *input = bitcoin::psbt::Input {
                    witness_utxo: input.witness_utxo.take(),
                    non_witness_utxo: input.non_witness_utxo.take(),
                    final_script_witness: Some(Witness::p2tr_key_spend(&signature)),
                    unknown: std::mem::take(&mut input.unknown),
                    proprietary: std::mem::take(&mut input.proprietary),
                    ..Default::default()
                };
            }
        }

        let complete = psbt.inputs.iter().all(is_final);
        let mut result = json!({
            "psbt": encode_psbt(&psbt).trim_end(),
            "signedInputs": signed,
            "complete": complete,
        });
        if complete {
            let transaction = psbt.extract_tx_unchecked_fee_rate();
            result["hex"] = bitcoin::consensus::encode::serialize_hex(&transaction).into();
            result["txid"] = transaction.compute_txid().to_string().into();
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::absolute::LockTime;
    use bitcoin::hashes::Hash;
    use bitcoin::key::TapTweak;
    use bitcoin::secp256k1::{Message, Secp256k1, XOnlyPublicKey};
    use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
    use bitcoin::{transaction, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut};
    use labcoat_core::ToolkitConfig;
    use labcoat_runtime::node::{DevSigner as _, Node};
    use labcoat_runtime::{IndexerModule, Runtime};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::str::FromStr;
    use std::sync::OnceLock;

    const FORBIDDEN: i64 = RpcError::FORBIDDEN;

    fn temporary(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "labcoat-dev-signer-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A keystore made the way `labcoat up` makes the built-in wallet,
    /// under `passphrase`.
    fn keystore(dir: &Path, passphrase: &str) -> PathBuf {
        let config = ToolkitConfig {
            wallet_file: dir.join("wallet.json"),
            ..ToolkitConfig::default()
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut provider =
                    labcoat_core::system::connect(&config, Some(passphrase.into()), false)
                        .await
                        .unwrap();
                labcoat_core::wallet::init(&mut provider, &config, None, Some(passphrase.into()))
                    .await
                    .unwrap();
            });
        config.wallet_file
    }

    /// One unlocked dev wallet for every test: creating and unlocking a
    /// keystore are both slow on purpose.
    fn signer() -> Arc<DevSigner> {
        static SIGNER: OnceLock<Arc<DevSigner>> = OnceLock::new();
        SIGNER
            .get_or_init(|| {
                let path = keystore(&temporary("wallet"), local_wallet::PASSPHRASE);
                Arc::new(DevSigner::at(&path))
            })
            .clone()
    }

    fn own_output(signer: &DevSigner, sats: u64) -> TxOut {
        let addresses = signer.addresses().unwrap();
        TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: ScriptBuf::from_hex(
                addresses["addresses"][0]["scriptPubKey"].as_str().unwrap(),
            )
            .unwrap(),
        }
    }

    fn foreign_output(sats: u64) -> TxOut {
        let secp = Secp256k1::new();
        let key = bitcoin::secp256k1::SecretKey::from_slice(&[7; 32]).unwrap();
        TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: ScriptBuf::new_p2tr(&secp, key.x_only_public_key(&secp).0, None),
        }
    }

    /// An unsigned PSBT spending `coins` to one output.
    fn unsigned(coins: &[(OutPoint, TxOut)], output: TxOut) -> Psbt {
        let mut psbt = Psbt::from_unsigned_tx(Transaction {
            version: transaction::Version::TWO,
            lock_time: LockTime::ZERO,
            input: coins
                .iter()
                .map(|(outpoint, _)| TxIn {
                    previous_output: *outpoint,
                    sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                    ..TxIn::default()
                })
                .collect(),
            output: vec![output],
        })
        .unwrap();
        for (input, (_, coin)) in psbt.inputs.iter_mut().zip(coins) {
            input.witness_utxo = Some(coin.clone());
        }
        psbt
    }

    fn outpoint(vout: u32) -> OutPoint {
        OutPoint::new(bitcoin::Txid::from_byte_array([9; 32]), vout)
    }

    fn base64(psbt: &Psbt) -> String {
        encode_psbt(psbt).trim_end().to_owned()
    }

    #[test]
    fn exists_only_on_labcoat_network() {
        let built_in = local_wallet::default_path(NetworkTarget::Labcoat);
        assert!(DevSigner::open(NetworkTarget::Labcoat, &built_in).is_ok());
        for network in [
            NetworkTarget::Regtest,
            NetworkTarget::Signet,
            NetworkTarget::Testnet,
            NetworkTarget::Mainnet,
        ] {
            // Not with the built-in wallet, and not with that network's own.
            for wallet in [built_in.clone(), local_wallet::default_path(network)] {
                let error = DevSigner::open(network, &wallet).err().unwrap();
                assert_eq!(error.code, "SIGNER_UNSUPPORTED");
                assert!(
                    error.message.contains("only on Labcoat Network"),
                    "{}",
                    error.message
                );
            }
        }
    }

    #[test]
    fn signs_with_no_keystore_but_the_built_in_one() {
        let dir = temporary("policy");
        for wallet in [
            dir.join("wallet.json"),
            PathBuf::from(".labcoat/wallet.json"),
            local_wallet::default_path(NetworkTarget::Signet),
            local_wallet::default_path(NetworkTarget::Mainnet),
        ] {
            let error = DevSigner::open(NetworkTarget::Labcoat, &wallet)
                .err()
                .unwrap();
            assert_eq!(error.code, "SIGNER_UNSUPPORTED");
            assert!(error.message.contains("built-in"), "{}", error.message);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_keystore_under_another_passphrase_does_not_open() {
        let dir = temporary("passphrase");
        let path = keystore(&dir, "a passphrase of the user's own");
        let error = DevSigner::at(&path).addresses().unwrap_err();
        assert_eq!(error.code, WALLET_ERROR);
        assert!(
            error.message.contains("fixed passphrase"),
            "{}",
            error.message
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_another_keystore_is_refused() {
        let signer = signer();
        let dir = temporary("link");
        let link = dir.join("labcoat.json");
        // Even to a keystore the fixed passphrase would open.
        std::os::unix::fs::symlink(&signer.wallet_file, &link).unwrap();
        let error = DevSigner::at(&link).addresses().unwrap_err();
        assert_eq!(error.code, WALLET_ERROR);
        assert!(error.message.contains("links"), "{}", error.message);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn says_when_the_wallet_does_not_exist_yet() {
        let dir = temporary("missing");
        let signer = DevSigner::at(&dir.join("labcoat.json"));
        assert_eq!(signer.addresses().unwrap_err().code, WALLET_NOT_FOUND);
        let psbt = unsigned(&[(outpoint(0), foreign_output(1_000))], foreign_output(500));
        assert_eq!(
            signer.sign_psbt(&base64(&psbt), true).unwrap_err().code,
            WALLET_NOT_FOUND
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn lists_the_keys_a_psbt_builder_needs() {
        let addresses = signer().addresses().unwrap();
        assert_eq!(addresses["network"], "regtest");
        assert_eq!(addresses["addresses"].as_array().unwrap().len(), 1);
        let primary = &addresses["addresses"][0];
        assert_eq!(primary["type"], "p2tr");
        assert_eq!(primary["derivationPath"], "m/86'/1'/0'/0/0");

        // The script pays to the internal key tweaked the BIP-86 way, and
        // the address is that script's.
        let secp = Secp256k1::new();
        let internal = XOnlyPublicKey::from_str(primary["internalKey"].as_str().unwrap()).unwrap();
        let (output_key, _) = internal.tap_tweak(&secp, None);
        assert_eq!(
            primary["outputKey"],
            output_key.to_x_only_public_key().to_string()
        );
        let script = ScriptBuf::new_p2tr(&secp, internal, None);
        assert_eq!(primary["scriptPubKey"], script.to_hex_string());
        assert_eq!(
            primary["address"],
            bitcoin::Address::from_script(&script, bitcoin::Network::Regtest)
                .unwrap()
                .to_string()
        );
    }

    #[test]
    fn signs_only_its_own_inputs_and_leaves_the_rest_untouched() {
        let signer = signer();
        let mut psbt = unsigned(
            &[
                (outpoint(0), foreign_output(40_000)),
                (outpoint(1), own_output(&signer, 60_000)),
            ],
            foreign_output(99_000),
        );
        // Something another signer left on its input.
        psbt.inputs[0].unknown.insert(
            bitcoin::psbt::raw::Key {
                type_value: 0xf0,
                key: b"note".to_vec(),
            },
            b"theirs".to_vec(),
        );
        let foreign = psbt.inputs[0].clone();

        for finalize in [false, true] {
            let result = signer.sign_psbt(&base64(&psbt), finalize).unwrap();
            assert_eq!(result["signedInputs"], json!([1]));
            assert_eq!(
                result["complete"], false,
                "one input is still someone else's"
            );
            assert!(result.get("hex").is_none() && result.get("txid").is_none());

            let signed = decode_psbt(result["psbt"].as_str().unwrap()).unwrap();
            assert_eq!(signed.unsigned_tx, psbt.unsigned_tx);
            assert_eq!(signed.inputs[0], foreign);
            let own = &signed.inputs[1];
            assert_eq!(own.tap_key_sig.is_some(), !finalize);
            assert_eq!(own.final_script_witness.is_some(), finalize);

            // The signature is a valid key-path spend of the wallet's coin.
            let signature = match (&own.tap_key_sig, &own.final_script_witness) {
                (Some(signature), None) => *signature,
                (None, Some(witness)) => {
                    bitcoin::taproot::Signature::from_slice(witness.nth(0).unwrap()).unwrap()
                }
                other => panic!("neither signed nor finalized: {other:?}"),
            };
            let prevouts: Vec<TxOut> = psbt
                .inputs
                .iter()
                .map(|input| input.witness_utxo.clone().unwrap())
                .collect();
            let sighash = SighashCache::new(&psbt.unsigned_tx)
                .taproot_key_spend_signature_hash(
                    1,
                    &Prevouts::All(&prevouts),
                    TapSighashType::Default,
                )
                .unwrap();
            let output_key =
                XOnlyPublicKey::from_slice(&prevouts[1].script_pubkey.as_bytes()[2..34]).unwrap();
            Secp256k1::new()
                .verify_schnorr(
                    &signature.signature,
                    &Message::from_digest(sighash.to_byte_array()),
                    &output_key,
                )
                .unwrap();
        }
    }

    #[test]
    fn a_psbt_with_none_of_its_inputs_comes_back_unchanged() {
        let psbt = unsigned(&[(outpoint(0), foreign_output(1_000))], foreign_output(500));
        let result = signer().sign_psbt(&base64(&psbt), true).unwrap();
        assert_eq!(result["signedInputs"], json!([]));
        assert_eq!(result["complete"], false);
        assert_eq!(result["psbt"], base64(&psbt));
    }

    #[test]
    fn refuses_what_it_cannot_sign_faithfully() {
        let signer = signer();
        assert_eq!(
            signer.sign_psbt("not a psbt", true).unwrap_err().code,
            DECODE_FAILED
        );

        let coin = (outpoint(0), own_output(&signer, 1_000));
        let mut bare = unsigned(std::slice::from_ref(&coin), foreign_output(500));
        bare.inputs[0].witness_utxo = None;
        let error = signer.sign_psbt(&base64(&bare), true).unwrap_err();
        assert_eq!(error.code, -8);
        assert!(error.message.contains("witness UTXO"), "{}", error.message);

        // It would sign with SIGHASH_DEFAULT, which is not what was asked.
        let mut single = unsigned(std::slice::from_ref(&coin), foreign_output(500));
        single.inputs[0].sighash_type = Some(TapSighashType::Single.into());
        let error = signer.sign_psbt(&base64(&single), true).unwrap_err();
        assert_eq!(error.code, -8);
        assert!(
            error.message.contains("SIGHASH_DEFAULT"),
            "{}",
            error.message
        );
    }

    /// POST one JSON-RPC request and return the response object.
    fn post(port: u16, headers: &str, method: &str, params: Value) -> Value {
        let body =
            json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "POST / HTTP/1.1\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let start = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap()
            + 4;
        serde_json::from_slice(&response[start..]).unwrap()
    }

    fn rpc(port: u16, method: &str, params: Value) -> Value {
        let headers = format!("Host: 127.0.0.1:{port}\r\n");
        let response = post(port, &headers, method, params);
        assert!(
            response.get("error").is_none_or(Value::is_null),
            "{method}: {response}"
        );
        response["result"].clone()
    }

    /// Serve a Labcoat Network with a stand-in indexer on a free port.
    fn serve(signer: Option<Arc<DevSigner>>) -> (u16, std::thread::JoinHandle<()>) {
        let module = IndexerModule::from_bytes(include_bytes!(
            "../../../scripts/network-smoke/stub-indexer.wat"
        ))
        .unwrap();
        let mut node = Node::new(Runtime::regtest(module).unwrap());
        if let Some(signer) = signer {
            node = node.with_dev_signer(signer);
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            labcoat_runtime::server::serve(Arc::new(node), listener).unwrap()
        });
        (port, server)
    }

    /// What a client does, over the endpoint alone: ask for the wallet's
    /// keys, fund it, build an unsigned PSBT, have it signed, broadcast the
    /// result, and mine it.
    fn spends_from_the_dev_wallet(port: u16) {
        let wallet = rpc(port, "labcoat_dev_addresses", json!([]));
        let primary = &wallet["addresses"][0];
        let address = primary["address"].as_str().unwrap();
        let script = ScriptBuf::from_hex(primary["scriptPubKey"].as_str().unwrap()).unwrap();

        // A coinbase output of the wallet's, old enough to spend.
        let mined = rpc(port, "generatetoaddress", json!([101, address]));
        let block = rpc(port, "getblock", json!([mined[0], 2]));
        let coinbase = &block["tx"][0];
        let coin = TxOut {
            value: Amount::from_btc(coinbase["vout"][0]["value"].as_f64().unwrap()).unwrap(),
            script_pubkey: script.clone(),
        };
        assert_eq!(
            coinbase["vout"][0]["scriptPubKey"]["hex"],
            script.to_hex_string()
        );
        let outpoint = OutPoint::new(coinbase["txid"].as_str().unwrap().parse().unwrap(), 0);

        let mut psbt = unsigned(
            &[(outpoint, coin.clone())],
            TxOut {
                value: coin.value - Amount::from_sat(1_000),
                script_pubkey: script,
            },
        );
        psbt.inputs[0].tap_internal_key =
            Some(XOnlyPublicKey::from_str(primary["internalKey"].as_str().unwrap()).unwrap());

        let signed = rpc(port, "labcoat_dev_sign_psbt", json!([base64(&psbt)]));
        assert_eq!(signed["signedInputs"], json!([0]));
        assert_eq!(signed["complete"], true);
        let txid = signed["txid"].as_str().unwrap();
        assert_eq!(txid, psbt.unsigned_tx.compute_txid().to_string());

        // Broadcasting puts it in the mempool, and it stays there until
        // the caller asks for a block.
        assert_eq!(
            rpc(port, "sendrawtransaction", json!([signed["hex"]])),
            txid
        );
        let pending = rpc(port, "getrawtransaction", json!([txid, true]));
        assert!(pending.get("confirmations").is_none(), "{pending}");
        assert!(rpc(port, "getrawmempool", json!([]))
            .as_array()
            .unwrap()
            .contains(&json!(txid)));

        rpc(port, "generatetoaddress", json!([1, address]));
        let confirmed = rpc(port, "getrawtransaction", json!([txid, true]));
        assert_eq!(confirmed["confirmations"], 1);
    }

    #[test]
    fn a_client_spends_from_the_dev_wallet_through_the_endpoint() {
        let (port, server) = serve(Some(signer()));
        assert_eq!(rpc(port, "labcoat_info", json!([]))["devSigner"], true);
        spends_from_the_dev_wallet(port);
        rpc(port, "stop", json!([]));
        server.join().unwrap();
    }

    /// The same, against the network `labcoat up` started:
    ///
    /// ```text
    /// labcoat up
    /// cargo test -p labcoat-cli a_client_spends_from_the_running -- --ignored
    /// ```
    ///
    /// `LABCOAT_RPC_PORT` names its port when it is not the default.
    #[test]
    #[ignore = "needs a running Labcoat Network; run `labcoat up` first"]
    fn a_client_spends_from_the_running_networks_dev_wallet() {
        let port = std::env::var("LABCOAT_RPC_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(18443);
        spends_from_the_dev_wallet(port);
    }

    #[test]
    fn only_this_machines_pages_reach_the_signer_over_http() {
        let (port, server) = serve(Some(signer()));
        let call = |headers: &str| {
            let response = post(port, headers, "labcoat_dev_addresses", json!([]));
            response["error"]["code"].as_i64()
        };
        // A script, a page on a local dev server, and that server's proxy.
        assert_eq!(call(""), None);
        assert_eq!(call(&format!("Host: 127.0.0.1:{port}\r\n")), None);
        assert_eq!(
            call("Host: localhost:5173\r\nOrigin: http://localhost:5173\r\n"),
            None
        );
        // A page from anywhere else, which a browser lets post here.
        assert_eq!(
            call(&format!(
                "Host: 127.0.0.1:{port}\r\nOrigin: https://example.com\r\n"
            )),
            Some(FORBIDDEN)
        );
        assert_eq!(
            call(&format!("Host: 127.0.0.1:{port}\r\nOrigin: null\r\n")),
            Some(FORBIDDEN)
        );
        // DNS rebinding: the attacker's name, resolved to this machine.
        assert_eq!(
            call(&format!(
                "Host: rebind.example.com:{port}\r\nOrigin: http://rebind.example.com:{port}\r\n"
            )),
            Some(FORBIDDEN)
        );
        assert_eq!(
            call(&format!("Host: rebind.example.com:{port}\r\n")),
            Some(FORBIDDEN)
        );
        // Nothing is signed for them either, and the rest of the endpoint
        // is unchanged.
        let psbt = unsigned(&[(outpoint(0), foreign_output(1_000))], foreign_output(500));
        let refused = post(
            port,
            "Host: 127.0.0.1\r\nOrigin: https://example.com\r\n",
            "labcoat_dev_sign_psbt",
            json!([base64(&psbt)]),
        );
        assert_eq!(refused["error"]["code"], FORBIDDEN);
        let read = post(
            port,
            "Host: 127.0.0.1\r\nOrigin: https://example.com\r\n",
            "getblockcount",
            json!([]),
        );
        assert_eq!(read["result"], 0);

        rpc(port, "stop", json!([]));
        server.join().unwrap();
    }

    #[test]
    fn a_network_without_a_dev_signer_has_no_such_methods() {
        let (port, server) = serve(None);
        assert_eq!(rpc(port, "labcoat_info", json!([]))["devSigner"], false);
        for method in ["labcoat_dev_addresses", "labcoat_dev_sign_psbt"] {
            let response = post(port, "", method, json!(["cHNidP8="]));
            assert_eq!(response["error"]["code"], -32601, "{response}");
        }
        rpc(port, "stop", json!([]));
        server.join().unwrap();
    }
}
