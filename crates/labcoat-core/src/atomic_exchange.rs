//! Build and settle one atomic two-wallet Alkane exchange.
//!
//! This is the local/native signer path used by integration tests and trusted
//! developer workflows. Both wallets sign the same PSBT with `SIGHASH_ALL`, so
//! neither token delivery nor payment can occur independently. Production RFQ
//! systems should exchange the PSBT between separate signer processes instead
//! of loading both keystores into one coordinator.

use crate::error::{LabcoatError, Result};
use crate::system::ToolkitConfig;
use alkanes_cli_common::alkanes::execute::EnhancedAlkanesExecutor;
pub use alkanes_cli_common::alkanes::types::AlkaneId;
use alkanes_cli_common::alkanes::types::{
    EnhancedExecuteParams, ExecutionState, InputRequirement, OrdinalsStrategy, OutputTarget,
    ProtostoneEdict, ProtostoneSpec, UtxoDataSource,
};
use alkanes_cli_common::provider::{ConcreteProvider, WalletState};
use alkanes_cli_common::traits::{BitcoinRpcProvider, WalletProvider};
use bip39::Mnemonic;
use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::consensus::encode::serialize_hex;
use bitcoin::hashes::Hash;
use bitcoin::key::{TapTweak, UntweakedKeypair};
use bitcoin::psbt::Psbt;
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::{Address, TapSighashType, Witness};
use serde::Serialize;
use std::str::FromStr;

const POST_BROADCAST_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomicExchangeRequest {
    /// Asset delivered by the seller to the buyer.
    pub offered: AlkaneId,
    pub offered_amount: u64,
    /// Asset delivered by the buyer to the seller.
    pub payment: AlkaneId,
    pub payment_amount: u64,
    pub seller_address: String,
    pub buyer_address: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomicExchangeOutcome {
    pub txid: String,
    pub fee: u64,
    pub offered_asset: String,
    pub offered_amount: u64,
    pub payment_asset: String,
    pub payment_amount: u64,
    pub status: &'static str,
}

impl AtomicExchangeRequest {
    pub fn validate(&self) -> Result<()> {
        if self.offered_amount == 0 || self.payment_amount == 0 {
            return Err(LabcoatError::new(
                "CONFIG_INVALID",
                "atomic exchange amounts must be greater than zero",
                "set both offered and payment amounts",
            ));
        }
        if self.offered == self.payment {
            return Err(LabcoatError::new(
                "CONFIG_INVALID",
                "atomic exchange assets must be different",
                "choose distinct offered and payment Alkane IDs",
            ));
        }
        if self.seller_address == self.buyer_address {
            return Err(LabcoatError::new(
                "CONFIG_INVALID",
                "seller and buyer addresses must be different",
                "use isolated wallets for both sides of the exchange",
            ));
        }
        Ok(())
    }

    fn params(&self, config: &ToolkitConfig) -> EnhancedExecuteParams {
        let trade = ProtostoneSpec {
            cellpack: None,
            edicts: vec![
                ProtostoneEdict {
                    alkane_id: self.offered.clone(),
                    amount: self.offered_amount,
                    target: OutputTarget::Output(0),
                },
                ProtostoneEdict {
                    alkane_id: self.payment.clone(),
                    amount: self.payment_amount,
                    target: OutputTarget::Output(1),
                },
            ],
            bitcoin_transfer: None,
            // Any excess from a selected token UTXO returns to the buyer. This
            // matters when the buyer's payment UTXO exceeds the quoted premium.
            pointer: Some(OutputTarget::Output(0)),
            refund: Some(OutputTarget::Output(0)),
        };

        EnhancedExecuteParams {
            fee_rate: config.fee_rate,
            to_addresses: vec![self.buyer_address.clone(), self.seller_address.clone()],
            // Buyer first makes the buyer the normal source of BTC fees; token
            // requirements still force selection of the seller's offered UTXO.
            from_addresses: Some(vec![
                self.buyer_address.clone(),
                self.seller_address.clone(),
            ]),
            change_address: Some(self.buyer_address.clone()),
            alkanes_change_address: Some(self.buyer_address.clone()),
            input_requirements: vec![
                InputRequirement::Alkanes {
                    block: self.offered.block,
                    tx: self.offered.tx,
                    amount: self.offered_amount,
                },
                InputRequirement::Alkanes {
                    block: self.payment.block,
                    tx: self.payment.tx,
                    amount: self.payment_amount,
                },
            ],
            protostones: vec![trade],
            envelope_data: None,
            raw_output: true,
            trace_enabled: false,
            mine_enabled: false,
            auto_confirm: true,
            ordinals_strategy: OrdinalsStrategy::Exclude,
            mempool_indexer: false,
            split_transactions: false,
            known_pending_tx_hexes: Vec::new(),
            prefetched_utxos: Vec::new(),
            excluded_utxos: Vec::new(),
            skip_diesel_mint: true,
            max_indexed_height: None,
            utxo_source: UtxoDataSource::default(),
        }
    }
}

pub async fn primary_address(provider: &ConcreteProvider) -> Result<String> {
    provider
        .get_address()
        .await
        .map_err(|error| LabcoatError::classify(error.into()))
}

/// Build one PSBT, sign only the inputs owned by each participant, and
/// broadcast only after every input has a signature.
pub async fn run(
    buyer: &mut ConcreteProvider,
    seller: &ConcreteProvider,
    config: &ToolkitConfig,
    request: AtomicExchangeRequest,
) -> Result<AtomicExchangeOutcome> {
    request.validate()?;
    let params = request.params(config);

    let state = {
        let mut executor = EnhancedAlkanesExecutor::new(buyer);
        executor
            .execute(params)
            .await
            .map_err(|error| LabcoatError::classify(error.into()))?
    };
    let ready = match state {
        ExecutionState::ReadyToSign(ready) if ready.split_psbt.is_none() => ready,
        ExecutionState::ReadyToSign(_) => {
            return Err(LabcoatError::new(
                "TOOLKIT_ERROR",
                "atomic exchange unexpectedly requires a split transaction",
                "use clean, non-inscribed exchange inputs",
            ));
        }
        other => {
            return Err(LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("atomic exchange produced unexpected execution state: {other:?}"),
                "retry with a simple non-envelope exchange",
            ));
        }
    };

    let mut psbt = ready.psbt;
    let seller_signed = sign_owned_p2tr_inputs(seller, &mut psbt)?;
    let buyer_signed = sign_owned_p2tr_inputs(buyer, &mut psbt)?;
    if seller_signed == 0 || buyer_signed == 0 {
        return Err(LabcoatError::new(
            "WALLET_ERROR",
            format!(
                "both participants must contribute inputs (seller signed {seller_signed}, buyer signed {buyer_signed})"
            ),
            "verify the wallet addresses and token balances",
        ));
    }

    let mut transaction = psbt
        .clone()
        .extract_tx()
        .map_err(|error| LabcoatError::classify(error.into()))?;
    for (index, input) in psbt.inputs.iter().enumerate() {
        let signature = input.tap_key_sig.as_ref().ok_or_else(|| {
            LabcoatError::new(
                "WALLET_ERROR",
                format!("no participant signed transaction input {index}"),
                "every input must belong to either the seller or buyer wallet",
            )
        })?;
        transaction.input[index].witness = Witness::p2tr_key_spend(signature);
    }

    let txid = buyer
        .broadcast_transaction(serialize_hex(&transaction))
        .await
        .map_err(|error| LabcoatError::classify(error.into()))?;

    if config.network.uses_regtest() {
        buyer
            .generate_to_address(1, &crate::execute::regtest_mining_address())
            .await
            .map_err(|error| LabcoatError::classify(error.into()))?;
        crate::sync::wait_for_indexer(buyer, POST_BROADCAST_SYNC_TIMEOUT).await?;
    }

    Ok(AtomicExchangeOutcome {
        txid,
        fee: ready.fee,
        offered_asset: request.offered.to_string(),
        offered_amount: request.offered_amount,
        payment_asset: request.payment.to_string(),
        payment_amount: request.payment_amount,
        status: "success",
    })
}

/// Sign only P2TR key-path inputs whose previous-output address belongs to
/// `provider`. Unknown inputs are intentionally left untouched for the other
/// participant instead of being treated as an error by the one-wallet signer.
fn sign_owned_p2tr_inputs(provider: &ConcreteProvider, psbt: &mut Psbt) -> Result<usize> {
    let (keystore, mnemonic) = match provider.get_wallet_state() {
        WalletState::Unlocked { keystore, mnemonic } => (keystore, mnemonic),
        _ => {
            return Err(LabcoatError::new(
                "WALLET_LOCKED",
                "atomic exchange participant wallet is not unlocked",
                "provide LABCOAT_WALLET_PASSPHRASE",
            ));
        }
    };
    let network = provider.get_network();
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let mnemonic = Mnemonic::parse_in(bip39::Language::English, mnemonic)
        .map_err(|error| LabcoatError::classify(error.into()))?;
    let root = Xpriv::new_master(network, &mnemonic.to_seed(""))
        .map_err(|error| LabcoatError::classify(error.into()))?;

    let prevouts = psbt
        .inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            input.witness_utxo.clone().ok_or_else(|| {
                LabcoatError::new(
                    "WALLET_ERROR",
                    format!("PSBT input {index} has no witness UTXO"),
                    "atomic exchange requires fully populated segwit PSBT inputs",
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
            return Err(LabcoatError::new(
                "WALLET_ERROR",
                format!("atomic exchange input {index} is not P2TR"),
                "use Labcoat's default P2TR wallet addresses",
            ));
        }
        let address = Address::from_script(&prevout.script_pubkey, network).map_err(|error| {
            LabcoatError::new(
                "WALLET_ERROR",
                format!("cannot decode exchange input {index} address: {error}"),
                "use a standard P2TR previous output",
            )
        })?;
        let Some(path) = find_p2tr_path(keystore, network, &address.to_string())? else {
            continue;
        };

        let derived = root
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

fn find_p2tr_path(
    keystore: &alkanes_cli_common::keystore::Keystore,
    network: bitcoin::Network,
    wanted: &str,
) -> Result<Option<DerivationPath>> {
    for chain in 0..=1 {
        for index in 0..1000 {
            let info = keystore
                .get_addresses(network, "p2tr", chain, index, 1)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> AtomicExchangeRequest {
        AtomicExchangeRequest {
            offered: AlkaneId { block: 4, tx: 1 },
            offered_amount: 100,
            payment: AlkaneId { block: 2, tx: 2 },
            payment_amount: 500,
            seller_address: "seller".into(),
            buyer_address: "buyer".into(),
        }
    }

    #[test]
    fn routes_both_assets_in_one_protostone() {
        let params = request().params(&ToolkitConfig::default());
        assert_eq!(params.to_addresses, ["buyer", "seller"]);
        assert_eq!(params.protostones.len(), 1);
        assert_eq!(params.protostones[0].edicts.len(), 2);
        assert_eq!(
            params.protostones[0].edicts[0].target,
            OutputTarget::Output(0)
        );
        assert_eq!(
            params.protostones[0].edicts[1].target,
            OutputTarget::Output(1)
        );
        assert_eq!(params.protostones[0].pointer, Some(OutputTarget::Output(0)));
    }

    #[test]
    fn rejects_zero_same_asset_and_same_wallet_trades() {
        let mut invalid = request();
        invalid.payment_amount = 0;
        assert!(invalid.validate().is_err());

        let mut invalid = request();
        invalid.payment = invalid.offered.clone();
        assert!(invalid.validate().is_err());

        let mut invalid = request();
        invalid.buyer_address = invalid.seller_address.clone();
        assert!(invalid.validate().is_err());
    }
}
