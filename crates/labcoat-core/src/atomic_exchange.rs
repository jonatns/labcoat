//! Build and settle one atomic two-wallet Alkane exchange.
//!
//! This is the local/native signer path used by integration tests and trusted
//! developer workflows. Both wallets sign the same PSBT with `SIGHASH_ALL`, so
//! neither token delivery nor payment can occur independently. Production RFQ
//! systems should exchange the PSBT between separate signer processes instead
//! of loading both keystores into one coordinator.

use crate::error::{LabcoatError, Result};
use crate::signer::{KeystoreSigner, Signer};
use crate::system::ToolkitConfig;
use alkanes_cli_common::alkanes::execute::EnhancedAlkanesExecutor;
pub use alkanes_cli_common::alkanes::types::AlkaneId;
use alkanes_cli_common::alkanes::types::{
    EnhancedExecuteParams, ExecutionState, InputRequirement, OrdinalsStrategy, OutputTarget,
    ProtostoneEdict, ProtostoneSpec, UtxoDataSource,
};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::{BitcoinRpcProvider, WalletProvider};
use bitcoin::consensus::encode::serialize_hex;
use bitcoin::Witness;
use serde::Serialize;

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
    let seller_signer = KeystoreSigner::from_provider(seller)?;
    let buyer_signer = KeystoreSigner::from_provider(buyer)?;
    let seller_signed = seller_signer.sign_psbt(&mut psbt).await?;
    let buyer_signed = buyer_signer.sign_psbt(&mut psbt).await?;
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
