//! Pluggable transaction signers.
//!
//! Every state-changing flow signs through the [`Signer`] trait instead of
//! reaching into the keystore directly. Two backends ship today:
//!
//! - [`KeystoreSigner`] — the classic in-process signer over the unlocked
//!   alkanes keystore (regtest default).
//! - [`PsbtFileSigner`] — writes the unsigned PSBT to a file and waits for a
//!   signed copy, so any external tool (an air-gapped machine, Sparrow, a
//!   hardware-wallet vendor app, or `labcoat wallet sign-psbt` on another
//!   keystore) can produce the signatures without this process ever holding
//!   key material.
//!
//! [`RemoteSignerAdapter`] bridges a [`Signer`] into upstream's
//! `RemoteSigner` seam: once installed via
//! `ConcreteProvider::with_remote_signer`, every signature the upstream
//! executor requests is routed here before the local keystore path is even
//! consulted.

use crate::error::{LabcoatError, Result};
use alkanes_cli_common::keystore::Keystore;
use alkanes_cli_common::provider::{ConcreteProvider, WalletState};
use bip39::Mnemonic;
use bitcoin::bip32::{DerivationPath, Fingerprint, Xpriv};
use bitcoin::hashes::Hash;
use bitcoin::key::{TapTweak, UntweakedKeypair};
use bitcoin::psbt::Psbt;
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::{Address, TapSighashType};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

/// What a signer backend can and cannot do. Policy checks consult this
/// instead of matching on concrete backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignerCaps {
    /// Safe to sign without a human approving each transaction (in-process
    /// keystore under the regtest dev policy). External backends get their
    /// approval at the external tool.
    pub unattended_ok: bool,
    /// Can produce taproot script-path signatures (needed for envelope
    /// reveal transactions). External PSBT tools generally cannot.
    pub script_path: bool,
}

/// Which signing backend a command should use. This replaces the bare
/// `Option<String>` passphrase that used to act as the de-facto signer
/// handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerSpec {
    /// In-process signing with the project keystore, unlocked by
    /// `passphrase`.
    Keystore { passphrase: Option<String> },
    /// External signing: unsigned PSBTs are parked in `dir` and the flow
    /// resumes when a `.signed.psbt` sibling appears. All signatures come
    /// from the external tool — the installed remote signer preempts every
    /// local signing path. `passphrase` is only used to satisfy upstream's
    /// `get_internal_key`, which at the pinned rev derives a PSBT metadata
    /// key from the mnemonic instead of the keystore xpub; once upstream
    /// derives it publicly this becomes unnecessary and the keystore stays
    /// locked throughout.
    PsbtFile {
        dir: PathBuf,
        passphrase: Option<String>,
    },
}

impl SignerSpec {
    /// Parse a CLI/toml spec string: `keystore` or `psbt-file:<dir>`.
    pub fn parse(spec: &str, passphrase: Option<String>) -> Result<Self> {
        let spec = spec.trim();
        if spec.is_empty() || spec == "keystore" {
            return Ok(Self::Keystore { passphrase });
        }
        if let Some(dir) = spec.strip_prefix("psbt-file:") {
            if dir.is_empty() {
                return Err(LabcoatError::new(
                    "CONFIG_INVALID",
                    "psbt-file signer needs a directory",
                    "use --signer psbt-file:<dir>",
                ));
            }
            return Ok(Self::PsbtFile {
                dir: PathBuf::from(dir),
                passphrase,
            });
        }
        Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!("unknown signer '{spec}'"),
            "use `keystore` or `psbt-file:<dir>`",
        ))
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Keystore { .. } => "keystore",
            Self::PsbtFile { .. } => "psbt-file",
        }
    }
}

/// A signing backend. `sign_psbt` signs only the inputs this signer owns and
/// leaves every other input untouched, so multiple signers can complete one
/// PSBT (the atomic exchange) and unknown inputs are never an error.
#[async_trait::async_trait(?Send)]
pub trait Signer: Send + Sync {
    fn backend_name(&self) -> &'static str;
    /// BIP-32 master fingerprint of the wallet this signer controls, for
    /// approval displays and PSBT key-origin matching.
    fn fingerprint(&self) -> Option<Fingerprint>;
    /// Receive addresses this signer can sign for.
    async fn addresses(&self) -> Result<Vec<String>>;
    /// Sign owned inputs in place; returns how many inputs were signed.
    async fn sign_psbt(&self, psbt: &mut Psbt) -> Result<usize>;
    fn capabilities(&self) -> SignerCaps;
}

/// In-process signer over an unlocked keystore. Extracted from the original
/// atomic-exchange signing code; P2TR key-path only, `SIGHASH_DEFAULT`.
pub struct KeystoreSigner {
    keystore: Keystore,
    root: Xpriv,
    network: bitcoin::Network,
    fingerprint: Option<Fingerprint>,
}

impl KeystoreSigner {
    /// Build from a provider whose wallet is `Unlocked`. Labcoat's own
    /// copies of the mnemonic and seed are zeroized on drop; the upstream
    /// provider still holds its plain-String mnemonic (accepted residual
    /// risk until upstream adopts zeroization).
    pub fn from_provider(provider: &ConcreteProvider) -> Result<Self> {
        let (keystore, mnemonic) = match provider.get_wallet_state() {
            WalletState::Unlocked { keystore, mnemonic } => {
                (keystore.clone(), zeroize::Zeroizing::new(mnemonic.clone()))
            }
            _ => {
                return Err(LabcoatError::new(
                    "WALLET_LOCKED",
                    "the keystore signer needs an unlocked wallet",
                    "provide LABCOAT_WALLET_PASSPHRASE",
                ));
            }
        };
        let network = provider.get_network();
        let mnemonic = Mnemonic::parse_in(bip39::Language::English, mnemonic.as_str())
            .map_err(|error| LabcoatError::classify(error.into()))?;
        let seed = zeroize::Zeroizing::new(mnemonic.to_seed(""));
        let root = Xpriv::new_master(network, seed.as_ref())
            .map_err(|error| LabcoatError::classify(error.into()))?;
        let fingerprint = Fingerprint::from_str(&keystore.master_fingerprint).ok();
        Ok(Self {
            keystore,
            root,
            network,
            fingerprint,
        })
    }

    /// Match a P2TR address to its derivation path by scanning the
    /// keystore's derived receive/change addresses.
    fn find_p2tr_path(&self, wanted: &str) -> Result<Option<DerivationPath>> {
        for chain in 0..=1 {
            for index in 0..1000 {
                let info = self
                    .keystore
                    .get_addresses(self.network, "p2tr", chain, index, 1)
                    .map_err(|error| LabcoatError::classify(error.into()))?;
                if let Some(info) = info.first().filter(|info| info.address == wanted) {
                    return DerivationPath::from_str(&info.derivation_path)
                        .map(Some)
                        .map_err(|error| LabcoatError::classify(error.into()));
                }
            }
        }
        Ok(None)
    }
}

#[async_trait::async_trait(?Send)]
impl Signer for KeystoreSigner {
    fn backend_name(&self) -> &'static str {
        "keystore"
    }

    fn fingerprint(&self) -> Option<Fingerprint> {
        self.fingerprint
    }

    async fn addresses(&self) -> Result<Vec<String>> {
        let addresses = self
            .keystore
            .get_addresses(self.network, "p2tr", 0, 0, 20)
            .map_err(|error| LabcoatError::classify(error.into()))?;
        Ok(addresses.into_iter().map(|info| info.address).collect())
    }

    async fn sign_psbt(&self, psbt: &mut Psbt) -> Result<usize> {
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let prevouts = psbt
            .inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                input.witness_utxo.clone().ok_or_else(|| {
                    LabcoatError::new(
                        "WALLET_ERROR",
                        format!("PSBT input {index} has no witness UTXO"),
                        "signing requires fully populated segwit PSBT inputs",
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let transaction = psbt.unsigned_tx.clone();
        let mut signed = 0;

        for index in 0..psbt.inputs.len() {
            if psbt.inputs[index].tap_key_sig.is_some() {
                continue;
            }
            let prevout = &prevouts[index];
            if !prevout.script_pubkey.is_p2tr() {
                // Not ours to judge: another signer may own this input.
                continue;
            }
            let address =
                Address::from_script(&prevout.script_pubkey, self.network).map_err(|error| {
                    LabcoatError::new(
                        "WALLET_ERROR",
                        format!("cannot decode input {index} address: {error}"),
                        "use a standard P2TR previous output",
                    )
                })?;
            let Some(path) = self.find_p2tr_path(&address.to_string())? else {
                continue;
            };

            let derived = self
                .root
                .derive_priv(&secp, &path)
                .map_err(|error| LabcoatError::classify(error.into()))?;
            let untweaked = UntweakedKeypair::from(derived.to_keypair(&secp));
            let tweaked = untweaked.tap_tweak(&secp, None);
            let sighash = SighashCache::new(&transaction)
                .taproot_key_spend_signature_hash(
                    index,
                    &Prevouts::All(&prevouts),
                    TapSighashType::Default,
                )
                .map_err(|error| LabcoatError::classify(error.into()))?;
            let message = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
            let signature = secp.sign_schnorr_no_aux_rand(&message, &tweaked.to_keypair());
            psbt.inputs[index].tap_key_sig = Some(bitcoin::taproot::Signature {
                signature,
                sighash_type: TapSighashType::Default,
            });
            signed += 1;
        }

        Ok(signed)
    }

    fn capabilities(&self) -> SignerCaps {
        SignerCaps {
            unattended_ok: true,
            script_path: false,
        }
    }
}

/// External signer over a shared directory: park the unsigned PSBT, wait for
/// the signed copy. Timeout via `LABCOAT_PSBT_TIMEOUT_SECS` (default 600).
pub struct PsbtFileSigner {
    dir: PathBuf,
    fingerprint: Option<Fingerprint>,
    addresses: Vec<String>,
    timeout: std::time::Duration,
}

impl PsbtFileSigner {
    /// Build from a provider whose keystore is loaded (locked is enough —
    /// the mnemonic is never needed here, only addresses).
    pub fn from_provider(provider: &ConcreteProvider, dir: PathBuf) -> Result<Self> {
        let keystore = provider
            .get_keystore()
            .map_err(|error| LabcoatError::classify(error.into()))?;
        let network = provider.get_network();
        let fingerprint = Fingerprint::from_str(&keystore.master_fingerprint).ok();
        let mut addresses = Vec::new();
        for chain in 0..=1 {
            let infos = keystore
                .get_addresses(network, "p2tr", chain, 0, 100)
                .map_err(|error| LabcoatError::classify(error.into()))?;
            addresses.extend(infos.into_iter().map(|info| info.address));
        }
        let timeout = std::env::var("LABCOAT_PSBT_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(std::time::Duration::from_secs)
            .unwrap_or(std::time::Duration::from_secs(600));
        Ok(Self {
            dir,
            fingerprint,
            addresses,
            timeout,
        })
    }

    fn request_paths(&self, psbt: &Psbt) -> (PathBuf, PathBuf) {
        let txid = psbt.unsigned_tx.compute_txid().to_string();
        let stem = &txid[..16.min(txid.len())];
        (
            self.dir.join(format!("{stem}.psbt")),
            self.dir.join(format!("{stem}.signed.psbt")),
        )
    }
}

#[async_trait::async_trait(?Send)]
impl Signer for PsbtFileSigner {
    fn backend_name(&self) -> &'static str {
        "psbt-file"
    }

    fn fingerprint(&self) -> Option<Fingerprint> {
        self.fingerprint
    }

    async fn addresses(&self) -> Result<Vec<String>> {
        Ok(self.addresses.clone())
    }

    async fn sign_psbt(&self, psbt: &mut Psbt) -> Result<usize> {
        std::fs::create_dir_all(&self.dir).map_err(|error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot create {}: {error}", self.dir.display()),
                "check permissions on the signer directory",
            )
        })?;
        let (request, response) = self.request_paths(psbt);
        let already_signed = count_signed_inputs(psbt);
        std::fs::write(&request, encode_psbt(psbt)).map_err(|error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot write {}: {error}", request.display()),
                "check permissions on the signer directory",
            )
        })?;
        eprintln!(
            "waiting for external signature: sign {} and save the result as {}",
            request.display(),
            response.display()
        );

        let deadline = std::time::Instant::now() + self.timeout;
        let signed_psbt = loop {
            if let Ok(raw) = std::fs::read_to_string(&response) {
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    break decode_psbt(trimmed)?;
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(LabcoatError::new(
                    "SIGNER_TIMEOUT",
                    format!(
                        "no signed PSBT appeared at {} within {}s",
                        response.display(),
                        self.timeout.as_secs()
                    ),
                    "sign the request PSBT with your external tool, or raise LABCOAT_PSBT_TIMEOUT_SECS",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        };

        if signed_psbt.unsigned_tx.compute_txid() != psbt.unsigned_tx.compute_txid() {
            return Err(LabcoatError::new(
                "SIGNER_MISMATCH",
                "the signed PSBT is for a different transaction than the request",
                "sign the exact request PSBT without changing inputs or outputs",
            ));
        }
        psbt.combine(signed_psbt)
            .map_err(|error| LabcoatError::classify(error.into()))?;
        std::fs::remove_file(&request).ok();
        std::fs::remove_file(&response).ok();
        Ok(count_signed_inputs(psbt).saturating_sub(already_signed))
    }

    fn capabilities(&self) -> SignerCaps {
        SignerCaps {
            unattended_ok: false,
            script_path: false,
        }
    }
}

fn count_signed_inputs(psbt: &Psbt) -> usize {
    psbt.inputs
        .iter()
        .filter(|input| {
            input.tap_key_sig.is_some()
                || input.final_script_witness.is_some()
                || !input.partial_sigs.is_empty()
                || !input.tap_script_sigs.is_empty()
        })
        .count()
}

/// Base64 (BIP-174 file convention) with a trailing newline.
pub fn encode_psbt(psbt: &Psbt) -> String {
    use base64::Engine;
    let mut encoded = base64::engine::general_purpose::STANDARD.encode(psbt.serialize());
    encoded.push('\n');
    encoded
}

/// Accept base64 (standard PSBT text) or hex.
pub fn decode_psbt(raw: &str) -> Result<Psbt> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .or_else(|_| hex::decode(raw))
        .map_err(|_| {
            LabcoatError::new(
                "SIGNER_MISMATCH",
                "signed PSBT file is neither base64 nor hex",
                "save the signed PSBT as base64 text",
            )
        })?;
    Psbt::deserialize(&bytes).map_err(|error| LabcoatError::classify(error.into()))
}

/// Bridge a [`Signer`] into upstream's `RemoteSigner` seam so the executor's
/// internal `sign_psbt` calls route through labcoat's signer.
pub struct RemoteSignerAdapter(pub Arc<dyn Signer>);

#[async_trait::async_trait(?Send)]
impl alkanes_cli_common::traits::RemoteSigner for RemoteSignerAdapter {
    async fn sign_psbt(
        &self,
        psbt: &Psbt,
        _addresses: &[String],
    ) -> alkanes_cli_common::Result<Psbt> {
        let mut signed = psbt.clone();
        self.0.sign_psbt(&mut signed).await.map_err(|error| {
            alkanes_cli_common::AlkanesError::Wallet(format!(
                "external signer '{}' failed: {error}",
                self.0.backend_name()
            ))
        })?;
        Ok(signed)
    }

    async fn get_addresses(&self) -> alkanes_cli_common::Result<Vec<String>> {
        self.0.addresses().await.map_err(|error| {
            alkanes_cli_common::AlkanesError::Wallet(format!(
                "external signer '{}' failed: {error}",
                self.0.backend_name()
            ))
        })
    }

    fn backend_name(&self) -> &'static str {
        self.0.backend_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signer_specs() {
        assert_eq!(
            SignerSpec::parse("keystore", Some("pw".into())).unwrap(),
            SignerSpec::Keystore {
                passphrase: Some("pw".into())
            }
        );
        assert_eq!(
            SignerSpec::parse("", None).unwrap(),
            SignerSpec::Keystore { passphrase: None }
        );
        assert_eq!(
            SignerSpec::parse("psbt-file:./psbts", None).unwrap(),
            SignerSpec::PsbtFile {
                dir: PathBuf::from("./psbts"),
                passphrase: None,
            }
        );
        assert!(SignerSpec::parse("psbt-file:", None).is_err());
        assert!(SignerSpec::parse("hardware", None).is_err());
    }

    #[test]
    fn psbt_text_round_trips_base64_and_hex() {
        let tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        };
        let psbt = Psbt::from_unsigned_tx(tx).unwrap();
        let encoded = encode_psbt(&psbt);
        let decoded = decode_psbt(encoded.trim()).unwrap();
        assert_eq!(decoded.serialize(), psbt.serialize());
        let decoded_hex = decode_psbt(&hex::encode(psbt.serialize())).unwrap();
        assert_eq!(decoded_hex.serialize(), psbt.serialize());
        assert!(decode_psbt("not-a-psbt").is_err());
    }
}
