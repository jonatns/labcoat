//! Plan and settle one atomic two-wallet Alkane exchange.
//!
//! V1 deliberately uses seller-last `SIGHASH_DEFAULT` signing. The buyer may
//! receive an unsigned/partially-signed PSBT, but the seller never releases a
//! fully signed transaction before broadcasting it.

use crate::error::{LabcoatError, Result};
use crate::signer::{decode_psbt, encode_psbt, KeystoreSigner, Signer};
use crate::system::ToolkitConfig;
use alkanes_cli_common::alkanes::execute::EnhancedAlkanesExecutor;
pub use alkanes_cli_common::alkanes::types::AlkaneId;
use alkanes_cli_common::alkanes::types::{
    EnhancedExecuteParams, ExecutionState, InputRequirement, OrdinalsStrategy, OutputTarget,
    PrefetchedAlkane, PrefetchedUtxo, ProtostoneEdict, ProtostoneSpec, UtxoDataSource,
};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::{AlkanesProvider, BitcoinRpcProvider, WalletProvider};
use bitcoin::consensus::encode::{serialize, serialize_hex};
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::{Address, OutPoint, ScriptBuf, TapSighashType, TxOut, Witness};
use ordinals::{Artifact, Runestone};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

const DUST_LIMIT: u64 = 546;
const INDEXER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const POST_BROADCAST_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const PLAN_TAG: &[u8] = b"Labcoat/ExchangePlan/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomicExchangeRequest {
    pub offered: AlkaneId,
    pub offered_amount: u64,
    pub payment: AlkaneId,
    pub payment_amount: u64,
    pub seller_address: String,
    pub buyer_address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeOwner {
    Buyer,
    Seller,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanAsset {
    pub block: u64,
    pub tx: u64,
    pub amount: u64,
}

impl PlanAsset {
    fn id(&self) -> AlkaneId {
        AlkaneId {
            block: self.block,
            tx: self.tx,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangePlanInput {
    pub outpoint: String,
    pub owner: ExchangeOwner,
    pub value: u64,
    pub script_pubkey: String,
    pub assets: Vec<PlanAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeOutputRole {
    BuyerAssets,
    SellerSettlement,
    BuyerBitcoinChange,
    Runestone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangePlanOutput {
    pub index: u32,
    pub role: ExchangeOutputRole,
    pub value: u64,
    pub script_pubkey: String,
    pub assets: Vec<PlanAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedTip {
    pub height: u64,
    pub block_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangePlanV1 {
    pub version: u8,
    pub chain_id: String,
    pub observed_tip: ObservedTip,
    pub request: AtomicExchangeRequest,
    pub inputs: Vec<ExchangePlanInput>,
    pub outputs: Vec<ExchangePlanOutput>,
    pub fee: u64,
    pub fee_rate: f32,
    pub unsigned_txid: String,
    pub psbt: String,
    pub plan_digest: String,
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

#[derive(Clone)]
struct Candidate {
    outpoint: OutPoint,
    output: TxOut,
    owner: ExchangeOwner,
    assets: Vec<PlanAsset>,
    confirmations: u32,
    frozen: bool,
    has_inscriptions: bool,
    has_runes: bool,
    is_coinbase: bool,
    block_height: Option<u64>,
}

impl AtomicExchangeRequest {
    pub fn validate(&self) -> Result<()> {
        if self.offered_amount == 0 || self.payment_amount == 0 {
            return Err(exchange_error(
                "EXCHANGE_PLAN_INVALID",
                "exchange amounts must be greater than zero",
            ));
        }
        if self.offered == self.payment {
            return Err(exchange_error(
                "EXCHANGE_PLAN_INVALID",
                "exchange assets must be different",
            ));
        }
        if self.seller_address == self.buyer_address {
            return Err(exchange_error(
                "EXCHANGE_INPUT_OWNERSHIP",
                "seller and buyer addresses must be different",
            ));
        }
        Ok(())
    }
}

fn exchange_error(code: &'static str, message: impl Into<String>) -> LabcoatError {
    LabcoatError::new(
        code,
        message,
        "rebuild the exchange plan from current wallet and chain state",
    )
}

pub async fn primary_address(provider: &ConcreteProvider) -> Result<String> {
    provider
        .get_address()
        .await
        .map_err(|error| LabcoatError::classify(error.into()))
}

fn tagged_hash(tag: &[u8], payload: &[u8]) -> [u8; 32] {
    let tag_hash = Sha256::digest(tag);
    let mut hasher = Sha256::new();
    hasher.update(tag_hash);
    hasher.update(tag_hash);
    hasher.update(payload);
    hasher.finalize().into()
}

fn plan_digest(
    chain_id: &str,
    tip: &ObservedTip,
    request: &AtomicExchangeRequest,
    psbt: &Psbt,
    inputs: &[ExchangePlanInput],
    outputs: &[ExchangePlanOutput],
) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(chain_id.as_bytes());
    bytes.extend_from_slice(&tip.height.to_le_bytes());
    bytes.extend_from_slice(tip.block_hash.as_bytes());
    bytes.extend_from_slice(&request.offered.block.to_le_bytes());
    bytes.extend_from_slice(&request.offered.tx.to_le_bytes());
    bytes.extend_from_slice(&request.offered_amount.to_le_bytes());
    bytes.extend_from_slice(&request.payment.block.to_le_bytes());
    bytes.extend_from_slice(&request.payment.tx.to_le_bytes());
    bytes.extend_from_slice(&request.payment_amount.to_le_bytes());
    bytes.extend_from_slice(request.seller_address.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(request.buyer_address.as_bytes());
    bytes.extend_from_slice(&serialize(&psbt.unsigned_tx));
    for input in &psbt.inputs {
        if let Some(prevout) = &input.witness_utxo {
            bytes.extend_from_slice(&serialize(prevout));
        }
    }
    for input in inputs {
        bytes.extend_from_slice(&(input.outpoint.len() as u32).to_le_bytes());
        bytes.extend_from_slice(input.outpoint.as_bytes());
        bytes.push(match input.owner {
            ExchangeOwner::Buyer => 0,
            ExchangeOwner::Seller => 1,
        });
        bytes.extend_from_slice(&input.value.to_le_bytes());
        bytes.extend_from_slice(&(input.script_pubkey.len() as u32).to_le_bytes());
        bytes.extend_from_slice(input.script_pubkey.as_bytes());
        bytes.extend_from_slice(&(input.assets.len() as u32).to_le_bytes());
        for asset in &input.assets {
            bytes.extend_from_slice(&asset.block.to_le_bytes());
            bytes.extend_from_slice(&asset.tx.to_le_bytes());
            bytes.extend_from_slice(&asset.amount.to_le_bytes());
        }
    }
    for output in outputs {
        bytes.extend_from_slice(&output.index.to_le_bytes());
        bytes.push(match output.role {
            ExchangeOutputRole::BuyerAssets => 0,
            ExchangeOutputRole::SellerSettlement => 1,
            ExchangeOutputRole::BuyerBitcoinChange => 2,
            ExchangeOutputRole::Runestone => 3,
        });
        bytes.extend_from_slice(&output.value.to_le_bytes());
        bytes.extend_from_slice(&(output.script_pubkey.len() as u32).to_le_bytes());
        bytes.extend_from_slice(output.script_pubkey.as_bytes());
        bytes.extend_from_slice(&(output.assets.len() as u32).to_le_bytes());
        for asset in &output.assets {
            bytes.extend_from_slice(&asset.block.to_le_bytes());
            bytes.extend_from_slice(&asset.tx.to_le_bytes());
            bytes.extend_from_slice(&asset.amount.to_le_bytes());
        }
    }
    hex::encode(tagged_hash(PLAN_TAG, &bytes))
}

async fn candidates_for(
    provider: &ConcreteProvider,
    address: &str,
    owner: ExchangeOwner,
) -> Result<Vec<Candidate>> {
    let utxos = WalletProvider::get_utxos(provider, true, Some(vec![address.to_string()]))
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    let holdings = AlkanesProvider::protorunes_by_address(provider, address, None, 1)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    let mut balances: BTreeMap<OutPoint, Vec<PlanAsset>> = BTreeMap::new();
    let mut indexed_outputs: BTreeMap<OutPoint, TxOut> = BTreeMap::new();
    for entry in holdings.balances {
        let mut assets = Vec::new();
        for (id, amount) in entry.balance_sheet.cached.balances {
            if amount == 0 {
                continue;
            }
            assets.push(PlanAsset {
                block: u64::try_from(id.block).map_err(|_| {
                    exchange_error("EXCHANGE_PLAN_INVALID", "Alkane block does not fit u64")
                })?,
                tx: u64::try_from(id.tx).map_err(|_| {
                    exchange_error("EXCHANGE_PLAN_INVALID", "Alkane tx does not fit u64")
                })?,
                amount: u64::try_from(amount).map_err(|_| {
                    exchange_error("EXCHANGE_PLAN_INVALID", "Alkane balance does not fit u64")
                })?,
            });
        }
        assets.sort_by_key(|asset| (asset.block, asset.tx));
        balances.insert(entry.outpoint, assets);
        indexed_outputs.insert(entry.outpoint, entry.output);
    }
    let mut result = Vec::new();
    for (outpoint, info) in utxos {
        let output = indexed_outputs.remove(&outpoint).or_else(|| {
            info.script_pubkey.clone().map(|script_pubkey| TxOut {
                value: bitcoin::Amount::from_sat(info.amount),
                script_pubkey,
            })
        });
        let Some(output) = output else { continue };
        result.push(Candidate {
            outpoint,
            output,
            owner,
            assets: balances.remove(&outpoint).unwrap_or_default(),
            confirmations: info.confirmations,
            frozen: info.frozen,
            has_inscriptions: info.has_inscriptions,
            has_runes: info.has_runes,
            is_coinbase: info.is_coinbase,
            block_height: info.block_height,
        });
    }
    Ok(result)
}

/// Discover Bitcoin-only fee inputs across the connected buyer wallet. Token
/// inputs remain selected from the buyer's explicit quote address; this wider
/// scan is only for UTXOs upstream already considers when funding fees.
async fn buyer_clean_candidates(provider: &ConcreteProvider) -> Result<Vec<Candidate>> {
    let utxos = WalletProvider::get_utxos(provider, true, None)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    let addresses: BTreeSet<String> = utxos
        .into_iter()
        .map(|(_, info)| info.address)
        .filter(|address| !address.is_empty())
        .collect();
    let mut candidates = Vec::new();
    for address in addresses {
        candidates.extend(candidates_for(provider, &address, ExchangeOwner::Buyer).await?);
    }
    candidates.retain(|candidate| candidate.assets.is_empty());
    Ok(candidates)
}

fn eligible(candidate: &Candidate, max_indexed_height: u64) -> bool {
    !(candidate.frozen
        || candidate.has_inscriptions
        || candidate.has_runes
        || candidate.is_coinbase && candidate.confirmations < 100)
        && candidate
            .block_height
            .is_none_or(|height| height <= max_indexed_height)
        && candidate.output.script_pubkey.is_p2tr()
}

fn only_asset(candidate: &Candidate, wanted: &AlkaneId) -> Option<u64> {
    (candidate.assets.len() == 1 && candidate.assets[0].id() == *wanted)
        .then_some(candidate.assets[0].amount)
}

fn select_token_inputs(
    mut candidates: Vec<Candidate>,
    wanted: &AlkaneId,
    amount: u64,
    max_indexed_height: u64,
) -> Result<(Vec<Candidate>, u64)> {
    candidates.retain(|candidate| {
        eligible(candidate, max_indexed_height) && only_asset(candidate, wanted).is_some()
    });
    candidates.sort_by(|a, b| {
        only_asset(b, wanted)
            .cmp(&only_asset(a, wanted))
            .then_with(|| a.outpoint.cmp(&b.outpoint))
    });
    let mut selected = Vec::new();
    let mut total = 0u64;
    for candidate in candidates {
        total = total
            .checked_add(only_asset(&candidate, wanted).unwrap())
            .ok_or_else(|| exchange_error("EXCHANGE_PLAN_INVALID", "asset selection overflow"))?;
        selected.push(candidate);
        if total >= amount {
            break;
        }
    }
    if total < amount {
        return Err(LabcoatError::new(
            "INSUFFICIENT_FUNDS",
            format!("owner has {total} spendable units but exchange requires {amount}"),
            "fund the participant wallet with a clean single-asset UTXO",
        ));
    }
    Ok((selected, total))
}

fn as_prefetched(candidate: &Candidate) -> PrefetchedUtxo {
    PrefetchedUtxo {
        outpoint: candidate.outpoint.to_string(),
        value: candidate.output.value.to_sat(),
        script_pubkey_hex: hex::encode(candidate.output.script_pubkey.as_bytes()),
        alkanes: Some(
            candidate
                .assets
                .iter()
                .map(|asset| PrefetchedAlkane {
                    block: asset.block as u128,
                    tx: asset.tx as u128,
                    amount: asset.amount.to_string(),
                })
                .collect(),
        ),
    }
}

fn owner_for_script(script: &ScriptBuf, seller: &ScriptBuf) -> Result<ExchangeOwner> {
    if script == seller {
        Ok(ExchangeOwner::Seller)
    } else if script.is_p2tr() {
        // Buyer fee inputs may use an internal wallet change script. Their
        // ownership is proven by the required valid buyer signature.
        Ok(ExchangeOwner::Buyer)
    } else {
        Err(exchange_error(
            "EXCHANGE_INPUT_OWNERSHIP",
            "exchange input is not a supported P2TR owner script",
        ))
    }
}

fn expected_output_assets(
    request: &AtomicExchangeRequest,
    offered_total: u64,
    payment_total: u64,
) -> (Vec<PlanAsset>, Vec<PlanAsset>) {
    let mut buyer = vec![PlanAsset {
        block: request.offered.block,
        tx: request.offered.tx,
        amount: request.offered_amount,
    }];
    if payment_total > request.payment_amount {
        buyer.push(PlanAsset {
            block: request.payment.block,
            tx: request.payment.tx,
            amount: payment_total - request.payment_amount,
        });
    }
    let mut seller = vec![PlanAsset {
        block: request.payment.block,
        tx: request.payment.tx,
        amount: request.payment_amount,
    }];
    if offered_total > request.offered_amount {
        seller.push(PlanAsset {
            block: request.offered.block,
            tx: request.offered.tx,
            amount: offered_total - request.offered_amount,
        });
    }
    (buyer, seller)
}

fn validate_runestone_edicts(
    psbt: &Psbt,
    request: &AtomicExchangeRequest,
    offered_total: u64,
    payment_total: u64,
) -> Result<()> {
    let runestone = match Runestone::decipher(&psbt.unsigned_tx) {
        Some(Artifact::Runestone(runestone)) => runestone,
        _ => {
            return Err(exchange_error(
                "EXCHANGE_ASSET_UNSAFE",
                "exchange transaction does not contain a valid Runestone",
            ))
        }
    };
    let protostones = alkanes_cli_common::Protostone::from_runestone(&runestone)
        .map_err(|e| exchange_error("EXCHANGE_ASSET_UNSAFE", e.to_string()))?;
    if protostones.len() != 1 {
        return Err(exchange_error(
            "EXCHANGE_ASSET_UNSAFE",
            "exchange must contain exactly one Alkane protostone",
        ));
    }
    let protostone = &protostones[0];
    if protostone.protocol_tag != 1
        || protostone.pointer != Some(0)
        || protostone.refund != Some(0)
        || protostone.burn.is_some()
        || protostone.from.is_some()
        || !protostone.message.is_empty()
    {
        return Err(exchange_error(
            "EXCHANGE_ASSET_UNSAFE",
            "exchange protostone routing fields differ from the fixed contract",
        ));
    }
    let mut expected = vec![
        (
            request.offered.block as u128,
            request.offered.tx as u128,
            request.offered_amount as u128,
            0u128,
        ),
        (
            request.payment.block as u128,
            request.payment.tx as u128,
            request.payment_amount as u128,
            1u128,
        ),
    ];
    if offered_total > request.offered_amount {
        expected.push((
            request.offered.block as u128,
            request.offered.tx as u128,
            (offered_total - request.offered_amount) as u128,
            1,
        ));
    }
    if payment_total > request.payment_amount {
        expected.push((
            request.payment.block as u128,
            request.payment.tx as u128,
            (payment_total - request.payment_amount) as u128,
            0,
        ));
    }
    expected.sort_unstable();
    let mut actual: Vec<_> = protostone
        .edicts
        .iter()
        .map(|edict| (edict.id.block, edict.id.tx, edict.amount, edict.output))
        .collect();
    actual.sort_unstable();
    if actual != expected {
        return Err(exchange_error(
            "EXCHANGE_ASSET_UNSAFE",
            "Runestone edicts do not exactly deliver the quote and owner surpluses",
        ));
    }
    Ok(())
}

/// Construct, but do not sign, a content-addressed exchange plan.
pub async fn build_exchange_plan(
    provider: &mut ConcreteProvider,
    config: &ToolkitConfig,
    request: AtomicExchangeRequest,
) -> Result<ExchangePlanV1> {
    request.validate()?;
    let max_indexed_height = crate::sync::wait_for_indexer(provider, INDEXER_TIMEOUT).await?;
    let chain_id = BitcoinRpcProvider::get_block_hash(provider, 1)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    let tip_height = BitcoinRpcProvider::get_block_count(provider)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    let tip = ObservedTip {
        height: tip_height,
        block_hash: BitcoinRpcProvider::get_block_hash(provider, tip_height)
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?,
    };
    let seller_address = Address::from_str(&request.seller_address)
        .map_err(|e| {
            exchange_error(
                "EXCHANGE_PLAN_INVALID",
                format!("invalid seller address: {e}"),
            )
        })?
        .require_network(provider.get_network())
        .map_err(|e| exchange_error("EXCHANGE_PLAN_INVALID", e.to_string()))?;
    let buyer_address = Address::from_str(&request.buyer_address)
        .map_err(|e| {
            exchange_error(
                "EXCHANGE_PLAN_INVALID",
                format!("invalid buyer address: {e}"),
            )
        })?
        .require_network(provider.get_network())
        .map_err(|e| exchange_error("EXCHANGE_PLAN_INVALID", e.to_string()))?;
    if !seller_address.script_pubkey().is_p2tr() || !buyer_address.script_pubkey().is_p2tr() {
        return Err(exchange_error(
            "EXCHANGE_PLAN_INVALID",
            "exchange participants must use P2TR addresses",
        ));
    }

    let seller_pool =
        candidates_for(provider, &request.seller_address, ExchangeOwner::Seller).await?;
    let buyer_pool = candidates_for(provider, &request.buyer_address, ExchangeOwner::Buyer).await?;
    let buyer_wallet_clean = buyer_clean_candidates(provider).await?;
    let (seller_selected, offered_total) = select_token_inputs(
        seller_pool.clone(),
        &request.offered,
        request.offered_amount,
        max_indexed_height,
    )?;
    let (buyer_payment, payment_total) = select_token_inputs(
        buyer_pool.clone(),
        &request.payment,
        request.payment_amount,
        max_indexed_height,
    )?;
    let selected_token_outpoints: BTreeSet<OutPoint> = seller_selected
        .iter()
        .chain(&buyer_payment)
        .map(|c| c.outpoint)
        .collect();
    let mut buyer_clean: Vec<Candidate> = buyer_pool
        .iter()
        .filter(|candidate| {
            eligible(candidate, max_indexed_height)
                && candidate.assets.is_empty()
                && !selected_token_outpoints.contains(&candidate.outpoint)
        })
        .cloned()
        .collect();
    for candidate in buyer_wallet_clean.iter().filter(|candidate| {
        eligible(candidate, max_indexed_height)
            && !selected_token_outpoints.contains(&candidate.outpoint)
    }) {
        if !buyer_clean
            .iter()
            .any(|existing| existing.outpoint == candidate.outpoint)
        {
            buyer_clean.push(candidate.clone());
        }
    }
    let mut allowed: Vec<Candidate> = seller_selected
        .iter()
        .chain(&buyer_payment)
        .cloned()
        .collect();
    allowed.extend(buyer_clean);
    let allowed_set: BTreeSet<OutPoint> = allowed.iter().map(|c| c.outpoint).collect();
    let excluded_utxos: Vec<String> = seller_pool
        .iter()
        .chain(&buyer_pool)
        .chain(&buyer_wallet_clean)
        .filter(|candidate| !allowed_set.contains(&candidate.outpoint))
        .map(|candidate| candidate.outpoint.to_string())
        .collect();
    let seller_input_sats = seller_selected
        .iter()
        .try_fold(0u64, |sum, c| sum.checked_add(c.output.value.to_sat()))
        .ok_or_else(|| exchange_error("EXCHANGE_PLAN_INVALID", "seller input value overflow"))?;
    let seller_output_sats = seller_input_sats.max(DUST_LIMIT);
    let mut edicts = vec![
        ProtostoneEdict {
            alkane_id: request.offered.clone(),
            amount: request.offered_amount,
            target: OutputTarget::Output(0),
        },
        ProtostoneEdict {
            alkane_id: request.payment.clone(),
            amount: request.payment_amount,
            target: OutputTarget::Output(1),
        },
    ];
    if offered_total > request.offered_amount {
        edicts.push(ProtostoneEdict {
            alkane_id: request.offered.clone(),
            amount: offered_total - request.offered_amount,
            target: OutputTarget::Output(1),
        });
    }
    if payment_total > request.payment_amount {
        edicts.push(ProtostoneEdict {
            alkane_id: request.payment.clone(),
            amount: payment_total - request.payment_amount,
            target: OutputTarget::Output(0),
        });
    }
    let params = EnhancedExecuteParams {
        fee_rate: config.fee_rate,
        to_addresses: vec![
            request.buyer_address.clone(),
            request.seller_address.clone(),
        ],
        from_addresses: Some(vec![
            request.buyer_address.clone(),
            request.seller_address.clone(),
        ]),
        change_address: Some(request.buyer_address.clone()),
        alkanes_change_address: Some(request.buyer_address.clone()),
        input_requirements: vec![
            InputRequirement::Alkanes {
                block: request.offered.block,
                tx: request.offered.tx,
                amount: request.offered_amount,
            },
            InputRequirement::Alkanes {
                block: request.payment.block,
                tx: request.payment.tx,
                amount: request.payment_amount,
            },
            InputRequirement::BitcoinOutput {
                amount: seller_output_sats,
                target: OutputTarget::Output(1),
            },
        ],
        protostones: vec![ProtostoneSpec {
            cellpack: None,
            edicts,
            bitcoin_transfer: None,
            pointer: Some(OutputTarget::Output(0)),
            refund: Some(OutputTarget::Output(0)),
        }],
        envelope_data: None,
        raw_output: true,
        trace_enabled: false,
        mine_enabled: false,
        auto_confirm: true,
        ordinals_strategy: OrdinalsStrategy::Exclude,
        mempool_indexer: false,
        split_transactions: false,
        known_pending_tx_hexes: Vec::new(),
        prefetched_utxos: allowed.iter().map(as_prefetched).collect(),
        excluded_utxos,
        skip_diesel_mint: true,
        max_indexed_height: Some(max_indexed_height),
        utxo_source: UtxoDataSource::default(),
    };
    let state = EnhancedAlkanesExecutor::new(provider)
        .execute(params)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    let ready = match state {
        ExecutionState::ReadyToSign(ready) if ready.split_psbt.is_none() => ready,
        _ => {
            return Err(exchange_error(
                "EXCHANGE_PLAN_INVALID",
                "exchange did not produce one signable PSBT",
            ))
        }
    };
    let psbt = ready.psbt;
    let actual: BTreeSet<OutPoint> = psbt
        .unsigned_tx
        .input
        .iter()
        .map(|input| input.previous_output)
        .collect();
    if !selected_token_outpoints.is_subset(&actual) {
        return Err(exchange_error(
            "EXCHANGE_INPUT_OWNERSHIP",
            format!(
                "upstream omitted owner-selected asset inputs (actual: {}; required assets: {})",
                actual
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
                selected_token_outpoints
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ));
    }
    let mut by_outpoint: BTreeMap<OutPoint, Candidate> =
        allowed.into_iter().map(|c| (c.outpoint, c)).collect();
    // Upstream may add a Bitcoin-only input from an internal buyer change
    // script after satisfying the caller's prefetched asset whitelist. Admit
    // only standard P2TR inputs that upstream represented as asset-free. A
    // seller-script input here would increase seller debit and is rejected.
    for (txin, psbt_input) in psbt.unsigned_tx.input.iter().zip(&psbt.inputs) {
        if by_outpoint.contains_key(&txin.previous_output) {
            continue;
        }
        let output = psbt_input.witness_utxo.clone().ok_or_else(|| {
            exchange_error(
                "EXCHANGE_INPUT_OWNERSHIP",
                "upstream-added Bitcoin input lacks witness metadata",
            )
        })?;
        let owner = owner_for_script(&output.script_pubkey, &seller_address.script_pubkey())?;
        if owner != ExchangeOwner::Buyer {
            return Err(exchange_error(
                "EXCHANGE_SELLER_DEBIT",
                "upstream selected an unplanned seller Bitcoin input",
            ));
        }
        by_outpoint.insert(
            txin.previous_output,
            Candidate {
                outpoint: txin.previous_output,
                output,
                owner,
                assets: Vec::new(),
                confirmations: 0,
                frozen: false,
                has_inscriptions: false,
                has_runes: false,
                is_coinbase: false,
                block_height: None,
            },
        );
    }
    let mut inputs = Vec::new();
    for txin in &psbt.unsigned_tx.input {
        let candidate = by_outpoint.get(&txin.previous_output).ok_or_else(|| {
            exchange_error(
                "EXCHANGE_INPUT_OWNERSHIP",
                "selected input has no owner metadata",
            )
        })?;
        inputs.push(ExchangePlanInput {
            outpoint: candidate.outpoint.to_string(),
            owner: candidate.owner,
            value: candidate.output.value.to_sat(),
            script_pubkey: hex::encode(candidate.output.script_pubkey.as_bytes()),
            assets: candidate.assets.clone(),
        });
    }
    let (buyer_assets, seller_assets) =
        expected_output_assets(&request, offered_total, payment_total);
    let mut outputs = Vec::new();
    for (index, output) in psbt.unsigned_tx.output.iter().enumerate() {
        let role = if index == 0 {
            ExchangeOutputRole::BuyerAssets
        } else if index == 1 {
            ExchangeOutputRole::SellerSettlement
        } else if output.script_pubkey.is_op_return() {
            ExchangeOutputRole::Runestone
        } else {
            ExchangeOutputRole::BuyerBitcoinChange
        };
        let assets = match role {
            ExchangeOutputRole::BuyerAssets => buyer_assets.clone(),
            ExchangeOutputRole::SellerSettlement => seller_assets.clone(),
            _ => Vec::new(),
        };
        outputs.push(ExchangePlanOutput {
            index: index as u32,
            role,
            value: output.value.to_sat(),
            script_pubkey: hex::encode(output.script_pubkey.as_bytes()),
            assets,
        });
    }
    let unsigned_txid = psbt.unsigned_tx.compute_txid().to_string();
    let digest = plan_digest(&chain_id, &tip, &request, &psbt, &inputs, &outputs);
    let plan = ExchangePlanV1 {
        version: 1,
        chain_id,
        observed_tip: tip,
        request,
        inputs,
        outputs,
        fee: ready.fee,
        fee_rate: config.fee_rate.unwrap_or(1.0),
        unsigned_txid,
        psbt: encode_psbt(&psbt).trim().to_string(),
        plan_digest: digest,
    };
    validate_exchange_plan(&plan, &psbt)?;
    Ok(plan)
}

pub fn validate_exchange_plan(plan: &ExchangePlanV1, psbt: &Psbt) -> Result<()> {
    if plan.version != 1 || psbt.unsigned_tx.compute_txid().to_string() != plan.unsigned_txid {
        return Err(exchange_error(
            "EXCHANGE_PLAN_MISMATCH",
            "PSBT transaction does not match the exchange plan",
        ));
    }
    if plan_digest(
        &plan.chain_id,
        &plan.observed_tip,
        &plan.request,
        psbt,
        &plan.inputs,
        &plan.outputs,
    ) != plan.plan_digest
    {
        return Err(exchange_error(
            "EXCHANGE_PLAN_MISMATCH",
            "exchange plan digest is invalid",
        ));
    }
    if psbt.inputs.len() != plan.inputs.len() || psbt.unsigned_tx.output.len() != plan.outputs.len()
    {
        return Err(exchange_error(
            "EXCHANGE_PLAN_MISMATCH",
            "PSBT input/output count differs from the plan",
        ));
    }
    let buyer = Address::from_str(&plan.request.buyer_address)
        .map_err(|e| exchange_error("EXCHANGE_PLAN_INVALID", e.to_string()))?
        .assume_checked()
        .script_pubkey();
    let seller = Address::from_str(&plan.request.seller_address)
        .map_err(|e| exchange_error("EXCHANGE_PLAN_INVALID", e.to_string()))?
        .assume_checked()
        .script_pubkey();
    for (index, (txin, input)) in psbt.unsigned_tx.input.iter().zip(&psbt.inputs).enumerate() {
        let prevout = input.witness_utxo.as_ref().ok_or_else(|| {
            exchange_error(
                "EXCHANGE_PLAN_MISMATCH",
                format!("input {index} lacks witness UTXO"),
            )
        })?;
        if !prevout.script_pubkey.is_p2tr()
            || txin.previous_output.to_string() != plan.inputs[index].outpoint
            || prevout.value.to_sat() != plan.inputs[index].value
            || hex::encode(prevout.script_pubkey.as_bytes()) != plan.inputs[index].script_pubkey
        {
            return Err(exchange_error(
                "EXCHANGE_PLAN_MISMATCH",
                format!("input {index} differs from plan metadata"),
            ));
        }
        if owner_for_script(&prevout.script_pubkey, &seller)? != plan.inputs[index].owner {
            return Err(exchange_error(
                "EXCHANGE_INPUT_OWNERSHIP",
                format!("input {index} owner differs from plan"),
            ));
        }
        if let Some(signature) = &input.tap_key_sig {
            if signature.sighash_type != TapSighashType::Default {
                return Err(exchange_error(
                    "EXCHANGE_SIGHASH_UNSUPPORTED",
                    "exchange signatures must use SIGHASH_DEFAULT",
                ));
            }
        }
    }
    for (index, output) in psbt.unsigned_tx.output.iter().enumerate() {
        let expected = &plan.outputs[index];
        if expected.index != index as u32
            || expected.value != output.value.to_sat()
            || expected.script_pubkey != hex::encode(output.script_pubkey.as_bytes())
        {
            return Err(exchange_error(
                "EXCHANGE_PLAN_MISMATCH",
                format!("output {index} differs from the plan"),
            ));
        }
    }
    if !(3..=4).contains(&plan.outputs.len()) {
        return Err(exchange_error(
            "EXCHANGE_PLAN_INVALID",
            "exchange must contain buyer assets, seller settlement, optional buyer change, and Runestone outputs",
        ));
    }
    if plan.outputs[0].role != ExchangeOutputRole::BuyerAssets
        || plan.outputs[0].value != DUST_LIMIT
        || psbt.unsigned_tx.output[0].script_pubkey != buyer
    {
        return Err(exchange_error(
            "EXCHANGE_PLAN_INVALID",
            "v0 must be the 546-sat buyer P2TR asset output",
        ));
    }
    let final_index = plan.outputs.len() - 1;
    if plan.outputs[final_index].role != ExchangeOutputRole::Runestone
        || psbt.unsigned_tx.output[final_index].value.to_sat() != 0
        || !psbt.unsigned_tx.output[final_index]
            .script_pubkey
            .is_op_return()
        || !plan.outputs[final_index].assets.is_empty()
    {
        return Err(exchange_error(
            "EXCHANGE_PLAN_INVALID",
            "the final output must be the zero-sat Runestone OP_RETURN",
        ));
    }
    if final_index == 3
        && (plan.outputs[2].role != ExchangeOutputRole::BuyerBitcoinChange
            || plan.outputs[2].value < DUST_LIMIT
            || psbt.unsigned_tx.output[2].script_pubkey != buyer
            || !plan.outputs[2].assets.is_empty())
    {
        return Err(exchange_error(
            "EXCHANGE_PLAN_INVALID",
            "v2 must be asset-free buyer P2TR change above dust",
        ));
    }
    let seller_inputs: u64 = plan
        .inputs
        .iter()
        .filter(|i| i.owner == ExchangeOwner::Seller)
        .map(|i| i.value)
        .sum();
    let seller_output = plan.outputs.get(1).ok_or_else(|| {
        exchange_error("EXCHANGE_PLAN_INVALID", "missing seller settlement output")
    })?;
    if seller_output.role != ExchangeOutputRole::SellerSettlement
        || seller_output.value != seller_inputs.max(DUST_LIMIT)
        || psbt.unsigned_tx.output[1].script_pubkey != seller
    {
        return Err(exchange_error(
            "EXCHANGE_SELLER_DEBIT",
            "seller settlement output does not return all seller input sats",
        ));
    }
    let mut offered_total = 0u64;
    let mut payment_total = 0u64;
    for input in &plan.inputs {
        for asset in &input.assets {
            if asset.id() == plan.request.offered && input.owner == ExchangeOwner::Seller {
                offered_total = offered_total.checked_add(asset.amount).ok_or_else(|| {
                    exchange_error("EXCHANGE_PLAN_INVALID", "offered asset total overflow")
                })?;
            } else if asset.id() == plan.request.payment && input.owner == ExchangeOwner::Buyer {
                payment_total = payment_total.checked_add(asset.amount).ok_or_else(|| {
                    exchange_error("EXCHANGE_PLAN_INVALID", "payment asset total overflow")
                })?;
            } else {
                return Err(exchange_error(
                    "EXCHANGE_ASSET_UNSAFE",
                    "an input carries an unrelated or wrong-owner Alkane",
                ));
            }
        }
    }
    if offered_total < plan.request.offered_amount || payment_total < plan.request.payment_amount {
        return Err(exchange_error(
            "EXCHANGE_PLAN_INVALID",
            "selected inputs do not fund the quoted asset amounts",
        ));
    }
    let (buyer_assets, seller_assets) =
        expected_output_assets(&plan.request, offered_total, payment_total);
    if plan.outputs[0].assets != buyer_assets || plan.outputs[1].assets != seller_assets {
        return Err(exchange_error(
            "EXCHANGE_ASSET_UNSAFE",
            "output asset allocation does not match quoted delivery and owner surplus",
        ));
    }
    validate_runestone_edicts(psbt, &plan.request, offered_total, payment_total)?;
    let input_value = plan.inputs.iter().try_fold(0u64, |sum, input| {
        sum.checked_add(input.value)
            .ok_or_else(|| exchange_error("EXCHANGE_PLAN_INVALID", "input value overflow"))
    })?;
    let output_value = plan.outputs.iter().try_fold(0u64, |sum, output| {
        sum.checked_add(output.value)
            .ok_or_else(|| exchange_error("EXCHANGE_PLAN_INVALID", "output value overflow"))
    })?;
    if input_value.checked_sub(output_value) != Some(plan.fee) {
        return Err(exchange_error(
            "EXCHANGE_PLAN_INVALID",
            "plan fee does not equal input value minus output value",
        ));
    }
    Ok(())
}

fn output_key(script: &ScriptBuf) -> Result<bitcoin::secp256k1::XOnlyPublicKey> {
    let bytes = script.as_bytes();
    if !script.is_p2tr() || bytes.len() != 34 {
        return Err(exchange_error(
            "EXCHANGE_PLAN_INVALID",
            "input is not a standard P2TR output",
        ));
    }
    bitcoin::secp256k1::XOnlyPublicKey::from_slice(&bytes[2..34]).map_err(|e| {
        exchange_error(
            "EXCHANGE_PLAN_INVALID",
            format!("invalid P2TR output key: {e}"),
        )
    })
}

fn verify_signatures(psbt: &Psbt, require_all: bool) -> Result<()> {
    let secp = bitcoin::secp256k1::Secp256k1::verification_only();
    let prevouts: Vec<TxOut> = psbt
        .inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
            input.witness_utxo.clone().ok_or_else(|| {
                exchange_error(
                    "EXCHANGE_PLAN_MISMATCH",
                    format!("input {i} lacks witness UTXO"),
                )
            })
        })
        .collect::<Result<_>>()?;
    for (index, input) in psbt.inputs.iter().enumerate() {
        let Some(signature) = &input.tap_key_sig else {
            if require_all {
                return Err(exchange_error(
                    "EXCHANGE_SIGNATURE_MISSING",
                    format!("input {index} is unsigned"),
                ));
            }
            continue;
        };
        if signature.sighash_type != TapSighashType::Default {
            return Err(exchange_error(
                "EXCHANGE_SIGHASH_UNSUPPORTED",
                format!("input {index} uses a non-default sighash"),
            ));
        }
        let sighash = SighashCache::new(&psbt.unsigned_tx)
            .taproot_key_spend_signature_hash(
                index,
                &Prevouts::All(&prevouts),
                TapSighashType::Default,
            )
            .map_err(|e| exchange_error("EXCHANGE_SIGNATURE_INVALID", e.to_string()))?;
        let message = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
        secp.verify_schnorr(
            &signature.signature,
            &message,
            &output_key(&prevouts[index].script_pubkey)?,
        )
        .map_err(|_| {
            exchange_error(
                "EXCHANGE_SIGNATURE_INVALID",
                format!("input {index} signature is invalid"),
            )
        })?;
    }
    Ok(())
}

/// Validate a buyer-signed plan, sign seller inputs, finalize, and optionally broadcast.
pub async fn settle_exchange(
    provider: &mut ConcreteProvider,
    signer: &dyn Signer,
    config: &ToolkitConfig,
    plan: &ExchangePlanV1,
    mut psbt: Psbt,
    broadcast: bool,
) -> Result<AtomicExchangeOutcome> {
    validate_exchange_plan(plan, &psbt)?;
    let chain_id = BitcoinRpcProvider::get_block_hash(provider, 1)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    if chain_id != plan.chain_id {
        return Err(exchange_error(
            "EXCHANGE_NETWORK_MISMATCH",
            "live chain identity differs from the plan",
        ));
    }
    let recorded_hash = BitcoinRpcProvider::get_block_hash(provider, plan.observed_tip.height)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    if recorded_hash != plan.observed_tip.block_hash {
        return Err(exchange_error(
            "EXCHANGE_TIP_STALE",
            "the plan's observed chain tip is no longer active",
        ));
    }
    // Segwit witness data does not affect txid. If a previous attempt
    // broadcast successfully but crashed before its quote ledger was updated,
    // the unsigned plan txid is enough to recover idempotently.
    let mut transaction_seen = broadcast
        && BitcoinRpcProvider::get_raw_transaction(provider, &plan.unsigned_txid, None)
            .await
            .is_ok();
    if broadcast && !transaction_seen {
        for vout in 0..plan.outputs.len() as u32 {
            if BitcoinRpcProvider::get_tx_out(provider, &plan.unsigned_txid, vout, true)
                .await
                .is_ok_and(|output| !output.is_null())
            {
                transaction_seen = true;
                break;
            }
        }
    }
    if transaction_seen {
        return Ok(AtomicExchangeOutcome {
            txid: plan.unsigned_txid.clone(),
            fee: plan.fee,
            offered_asset: plan.request.offered.to_string(),
            offered_amount: plan.request.offered_amount,
            payment_asset: plan.request.payment.to_string(),
            payment_amount: plan.request.payment_amount,
            status: "success",
        });
    }
    for input in &plan.inputs {
        let outpoint = OutPoint::from_str(&input.outpoint)
            .map_err(|e| exchange_error("EXCHANGE_PLAN_INVALID", e.to_string()))?;
        let live = BitcoinRpcProvider::get_tx_out(
            provider,
            &outpoint.txid.to_string(),
            outpoint.vout,
            true,
        )
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
        if live.is_null() {
            return Err(exchange_error(
                "EXCHANGE_INPUT_SPENT",
                format!("input {} is already spent", input.outpoint),
            ));
        }
    }
    for (index, metadata) in plan.inputs.iter().enumerate() {
        if metadata.owner == ExchangeOwner::Buyer && psbt.inputs[index].tap_key_sig.is_none() {
            return Err(exchange_error(
                "EXCHANGE_SIGNATURE_MISSING",
                format!("buyer input {index} is unsigned"),
            ));
        }
    }
    verify_signatures(&psbt, false)?;
    let seller_expected = plan
        .inputs
        .iter()
        .filter(|input| input.owner == ExchangeOwner::Seller)
        .count();
    let seller_signed = signer.sign_psbt(&mut psbt).await?;
    if seller_signed != seller_expected {
        return Err(exchange_error(
            "EXCHANGE_SIGNATURE_MISSING",
            format!("seller signed {seller_signed} of {seller_expected} expected inputs"),
        ));
    }
    verify_signatures(&psbt, true)?;
    for input in &mut psbt.inputs {
        let signature = input.tap_key_sig.ok_or_else(|| {
            exchange_error(
                "EXCHANGE_SIGNATURE_MISSING",
                "all exchange inputs must be signed",
            )
        })?;
        input.final_script_witness = Some(Witness::p2tr_key_spend(&signature));
    }
    let transaction = psbt
        .clone()
        .extract_tx()
        .map_err(|e| LabcoatError::classify(e.into()))?;
    let txid = transaction.compute_txid().to_string();
    if broadcast {
        provider
            .broadcast_transaction(serialize_hex(&transaction))
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
        if config.network.uses_regtest() {
            provider
                .generate_to_address(1, &crate::execute::regtest_mining_address())
                .await
                .map_err(|e| LabcoatError::classify(e.into()))?;
            crate::sync::wait_for_indexer(provider, POST_BROADCAST_SYNC_TIMEOUT).await?;
        }
    }
    Ok(AtomicExchangeOutcome {
        txid,
        fee: plan.fee,
        offered_asset: plan.request.offered.to_string(),
        offered_amount: plan.request.offered_amount,
        payment_asset: plan.request.payment.to_string(),
        payment_amount: plan.request.payment_amount,
        status: if broadcast { "success" } else { "ready" },
    })
}

/// Compatibility coordinator for trusted regtest workflows.
pub async fn run(
    buyer: &mut ConcreteProvider,
    seller: &ConcreteProvider,
    config: &ToolkitConfig,
    request: AtomicExchangeRequest,
) -> Result<AtomicExchangeOutcome> {
    let plan = build_exchange_plan(buyer, config, request).await?;
    let mut psbt = decode_psbt(&plan.psbt)?;
    let buyer_signer = KeystoreSigner::from_provider(buyer)?;
    if buyer_signer.sign_psbt(&mut psbt).await? == 0 {
        return Err(exchange_error(
            "EXCHANGE_SIGNATURE_MISSING",
            "buyer signed no exchange inputs",
        ));
    }
    let seller_signer = KeystoreSigner::from_provider(seller)?;
    settle_exchange(buyer, &seller_signer, config, &plan, psbt, true).await
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

    #[test]
    fn output_asset_change_is_partitioned_by_owner() {
        let (buyer, seller) = expected_output_assets(&request(), 125, 700);
        assert_eq!(
            buyer[1],
            PlanAsset {
                block: 2,
                tx: 2,
                amount: 200
            }
        );
        assert_eq!(
            seller[1],
            PlanAsset {
                block: 4,
                tx: 1,
                amount: 25
            }
        );
    }

    #[test]
    fn plan_hash_is_domain_separated() {
        assert_ne!(tagged_hash(PLAN_TAG, b"x"), tagged_hash(b"other", b"x"));
    }
}
