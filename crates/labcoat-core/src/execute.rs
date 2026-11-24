//! Execute (call) and deploy against Labcoat Network or an external network, via the pinned
//! alkanes-rs executor — commit/reveal envelope deploys included.

use crate::error::{LabcoatError, Result};
use crate::system::ToolkitConfig;
use alkanes_cli_common::alkanes::execute::EnhancedAlkanesExecutor;
pub use alkanes_cli_common::alkanes::types::InputRequirement;
use alkanes_cli_common::alkanes::types::{
    EnhancedExecuteParams, EnhancedExecuteResult, ExecutionState, OrdinalsStrategy,
    ReadyToSignRevealTx, UtxoDataSource,
};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::{BitcoinRpcProvider, WalletProvider};
use serde::Serialize;
use std::io::IsTerminal;
use std::str::FromStr;

const POST_BROADCAST_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteOutcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_txid: Option<String>,
    pub txid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_fee: Option<u64>,
    pub fee: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alkanes_id: Option<String>,
    /// Every Alkane the transaction created, in trace order (deploys).
    #[serde(skip)]
    pub created: Vec<String>,
    pub traces: Option<Vec<serde_json::Value>>,
}

/// Transaction-shaping options shared by call and deploy. The defaults
/// reproduce the historical behavior: no extra inputs, protostone outputs
/// to the wallet's primary address, pointer/refund at output 0, no edicts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TxOptions {
    /// Comma-separated input requirements: alkanes `block:tx:amount`
    /// (amount 0 means all) or bitcoin `B:sats` / `B:sats:vN`.
    pub inputs: Option<String>,
    /// Recipient of the protostone outputs (defaults to the wallet's
    /// primary address).
    pub to: Option<String>,
    /// Pointer target `vN` (physical output) or `pN` (protostone).
    pub pointer: Option<String>,
    /// Refund target `vN`/`pN`; defaults to the pointer target.
    pub refund: Option<String>,
    /// Edicts appended to the protostone, each `block:tx:amount:target`.
    pub edicts: Vec<String>,
}

impl TxOptions {
    /// Parse the `inputs` string into upstream requirements.
    pub fn input_requirements(&self) -> Result<Vec<InputRequirement>> {
        let Some(spec) = self.inputs.as_deref() else {
            return Ok(Vec::new());
        };
        alkanes_cli_common::alkanes::parsing::parse_input_requirements(spec).map_err(|e| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!("bad --inputs '{spec}': {e}"),
                "expected comma-separated block:tx:amount (alkanes) or B:sats entries",
            )
        })
    }
}

/// Build the standard cellpack protostone spec string:
/// `[block,tx,opcode,args…]:v0:v0` (pointer/refund to output 0).
pub fn cellpack_spec(block: u128, tx: u128, opcode: u128, args: &[u128]) -> String {
    let mut inputs = vec![opcode];
    inputs.extend_from_slice(args);
    spec_with_options(block, tx, &inputs, &TxOptions::default())
}

/// Build a protostone spec with explicit routing and edicts. `cellpack_inputs`
/// is the full input stream after the target (opcode first for calls and
/// constructors); an empty stream produces the bare `[block,tx]` form.
///
/// When the options require alkane inputs, an edict-only splitter protostone
/// is prepended: the protorune runtime auto-allocates every input alkane to
/// the first protostone, so the splitter forwards exactly the required
/// amounts to the cellpack protostone (`p1`) and returns any excess to
/// output 0 — the same place refunds land. Without it the contract would
/// receive whole UTXO balances (the upstream executor's alkanes-change
/// handling is disabled), and strict contracts reject the excess. Caller
/// `pN` references are shifted to account for the prepended protostone.
pub fn spec_with_options(
    block: u128,
    tx: u128,
    cellpack_inputs: &[u128],
    options: &TxOptions,
) -> String {
    let mut inputs = vec![block.to_string(), tx.to_string()];
    inputs.extend(cellpack_inputs.iter().map(|a| a.to_string()));

    let required_alkanes: Vec<(u128, u128, u128)> = options
        .input_requirements()
        .map(|requirements| {
            requirements
                .iter()
                .filter_map(|r| match r {
                    InputRequirement::Alkanes { block, tx, amount } => {
                        Some((*block as u128, *tx as u128, *amount as u128))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let split = !required_alkanes.is_empty();

    let shift = |target: &str| -> String {
        if split {
            shift_protostone_ref(target)
        } else {
            target.to_string()
        }
    };
    let pointer = shift(options.pointer.as_deref().unwrap_or("v0"));
    let refund = shift(
        options
            .refund
            .as_deref()
            .or(options.pointer.as_deref())
            .unwrap_or("v0"),
    );

    let mut spec = String::new();
    if split {
        spec.push_str("v0:v0");
        for (b, t, amount) in &required_alkanes {
            spec.push_str(&format!(":[{b}:{t}:{amount}:p1]"));
        }
        spec.push(',');
    }
    spec.push_str(&format!("[{}]:{}:{}", inputs.join(","), pointer, refund));
    for edict in &options.edicts {
        let edict = edict.trim();
        let edict = edict
            .strip_prefix('[')
            .and_then(|e| e.strip_suffix(']'))
            .unwrap_or(edict);
        let edict = if split {
            shift_edict_target(edict)
        } else {
            edict.to_string()
        };
        spec.push_str(&format!(":[{edict}]"));
    }
    spec
}

/// Shift a `pN` protostone reference one slot for the prepended splitter;
/// `vN` output references are untouched.
fn shift_protostone_ref(target: &str) -> String {
    if let Some(n) = target.strip_prefix('p').and_then(|n| n.parse::<u32>().ok()) {
        format!("p{}", n + 1)
    } else {
        target.to_string()
    }
}

/// Shift the target (last `:` segment) of a `block:tx:amount:target` edict.
fn shift_edict_target(edict: &str) -> String {
    match edict.rsplit_once(':') {
        Some((head, target)) => format!("{head}:{}", shift_protostone_ref(target)),
        None => edict.to_string(),
    }
}

/// Parse-check a spec through the upstream grammar without executing it.
/// Lets dry runs reject bad `--pointer`/`--refund`/`--edict` values with the
/// same error the broadcast path would produce.
pub fn validate_spec(spec: &str) -> Result<()> {
    alkanes_cli_common::alkanes::parsing::parse_protostones(spec)
        .map(|_| ())
        .map_err(|e| {
            LabcoatError::new(
                "ENVELOPE_INVALID",
                format!("bad protostone spec '{spec}': {e}"),
                "pointer/refund must be vN or pN; edicts must be block:tx:amount:target",
            )
        })
}

/// One transaction for [`run`]: the spec, its payload, and its shaping.
pub struct ExecuteRequest<'a> {
    pub spec: &'a str,
    /// Wasm envelope for deploys (commit/reveal); None for calls.
    pub envelope: Option<Vec<u8>>,
    pub to_address: String,
    pub fee_rate: Option<f32>,
    pub max_indexed_height: Option<u64>,
    pub input_requirements: Vec<InputRequirement>,
}

pub(crate) enum Broadcast<'a> {
    Commit(&'a str),
    Execute(&'a str),
}

pub(crate) type BroadcastObserver<'a> = dyn FnMut(Broadcast<'_>) -> Result<()> + 'a;

/// Run the executor with a cellpack spec, optional envelope (deploy), and
/// standard labcoat behavior: trace, auto-mine on regtest, UTXOs filtered to
/// the indexer height.
///
/// Signing goes through the pausable `execute()` state machine instead of
/// the opaque `execute_full()`: every transaction (single, commit, reveal)
/// is previewed and approved before it is signed and broadcast, and a
/// pending reveal is persisted to `.labcoat/pending/` between the commit
/// broadcast and the reveal so an interrupted deploy is recoverable.
pub async fn run(
    provider: &mut ConcreteProvider,
    config: &ToolkitConfig,
    request: ExecuteRequest<'_>,
) -> Result<EnhancedExecuteResult> {
    run_with_broadcast(provider, config, request, None).await
}

pub(crate) async fn run_with_broadcast(
    provider: &mut ConcreteProvider,
    config: &ToolkitConfig,
    request: ExecuteRequest<'_>,
    mut observe: Option<&mut BroadcastObserver<'_>>,
) -> Result<EnhancedExecuteResult> {
    let protostones = alkanes_cli_common::alkanes::parsing::parse_protostones(request.spec)
        .map_err(|e| {
            LabcoatError::new(
                "ENVELOPE_INVALID",
                format!("bad protostone spec '{}': {}", request.spec, e),
                "expected [block,tx,opcode,args...]:v0:v0",
            )
        })?;

    if request.envelope.is_some() && provider.has_remote_signer() {
        return Err(LabcoatError::new(
            "SIGNER_UNSUPPORTED",
            "contract deploys need a taproot script-path signature for the reveal, which external signers cannot produce yet",
            "use the keystore signer for deploys; external-signer deploys arrive with hardware-wallet support",
        ));
    }

    let mine_enabled = config.network.uses_regtest();
    let required_alkanes: std::collections::BTreeSet<(u128, u128)> = request
        .input_requirements
        .iter()
        .filter_map(|r| match r {
            InputRequirement::Alkanes { block, tx, .. } => Some((*block as u128, *tx as u128)),
            _ => None,
        })
        .collect();
    let (mut excluded_utxos, spendable) =
        protected_outpoints(provider, mine_enabled, &required_alkanes).await?;
    if request.envelope.is_some() {
        let bitcoin_requirements: u64 = request
            .input_requirements
            .iter()
            .filter_map(|requirement| match requirement {
                InputRequirement::Bitcoin { amount } => Some(*amount),
                _ => None,
            })
            .sum();
        excluded_utxos.extend(envelope_funding_exclusions(
            &spendable,
            DUST + bitcoin_requirements + UPSTREAM_REVEAL_BUFFER + COMMIT_HEADROOM,
        ));
    }

    let params = EnhancedExecuteParams {
        fee_rate: request.fee_rate.or(config.fee_rate),
        to_addresses: vec![request.to_address],
        from_addresses: None,
        change_address: None,
        alkanes_change_address: None,
        input_requirements: request.input_requirements,
        protostones,
        envelope_data: request.envelope,
        raw_output: true,
        // Labcoat owns the regtest mine/sync/trace sequence. Upstream traces
        // through bitcoind getrawtransaction, but Qubitcoin intentionally
        // serves historical transaction hex through its Esplora secondary.
        trace_enabled: false,
        mine_enabled: false,
        // Labcoat runs its own approval gate; the upstream stdin prompt
        // stays disabled.
        auto_confirm: true,
        ordinals_strategy: OrdinalsStrategy::default(),
        mempool_indexer: false,
        split_transactions: false,
        known_pending_tx_hexes: Vec::new(),
        prefetched_utxos: Vec::new(),
        excluded_utxos,
        // No implicit DIESEL mint: it would land on the same pointer output
        // as the transaction's own tokens, contaminating every minted UTXO
        // with a stray asset that strict contracts then reject as incoming.
        skip_diesel_mint: true,
        max_indexed_height: request.max_indexed_height,
        utxo_source: UtxoDataSource::default(),
    };

    let wallet_fingerprint = provider
        .get_keystore()
        .ok()
        .map(|keystore| keystore.master_fingerprint.clone());

    let result = {
        let mut executor = EnhancedAlkanesExecutor::new(provider);
        let state = executor
            .execute(params.clone())
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;

        match state {
            ExecutionState::Complete(result) => result,
            ExecutionState::ReadyToSign(tx) => {
                approve_transaction(
                    config,
                    &wallet_fingerprint,
                    &preview("transaction", &tx.psbt, tx.fee, config, &wallet_fingerprint),
                )?;
                match observe.as_deref_mut() {
                    Some(observe) => {
                        broadcast_signed(provider, tx.psbt, tx.fee, None, observe).await?
                    }
                    None => executor
                        .resume_execution(tx, &params)
                        .await
                        .map_err(|e| LabcoatError::classify(e.into()))?,
                }
            }
            ExecutionState::ReadyToSignCommit(mut commit) => {
                prefund_reveal_from_commit_change(&mut commit);
                approve_transaction(
                    config,
                    &wallet_fingerprint,
                    &preview(
                        "commit (envelope funding)",
                        &commit.psbt,
                        commit.fee,
                        config,
                        &wallet_fingerprint,
                    ),
                )?;
                // The native commit signer adds only witnesses for these
                // input types, so its funding txid is known before submission.
                if let Some(observe) = observe.as_deref_mut() {
                    let txid = commit_txid(&commit.psbt)?;
                    observe(Broadcast::Commit(&txid))?;
                }
                let next = executor
                    .resume_commit_execution(commit)
                    .await
                    .map_err(|e| LabcoatError::classify(e.into()))?;
                let ExecutionState::ReadyToSignReveal(reveal) = next else {
                    return Err(LabcoatError::new(
                        "TOOLKIT_ERROR",
                        format!("unexpected state after commit broadcast: {next:?}"),
                        "report this as a Labcoat bug",
                    ));
                };
                // Keep the prepared reveal until broadcast. A separate deploy
                // can approve and send it if the previous one stopped here.
                let pending = persist_pending_reveal(config, &reveal);
                approve_reveal(config, &wallet_fingerprint, &reveal)?;
                let result = match observe {
                    Some(observe) => {
                        broadcast_signed(
                            provider,
                            reveal.psbt,
                            reveal.fee,
                            Some((reveal.commit_txid, reveal.commit_fee)),
                            observe,
                        )
                        .await?
                    }
                    None => executor
                        .resume_reveal_execution(reveal)
                        .await
                        .map_err(|e| LabcoatError::classify(e.into()))?,
                };
                if let Some(pending) = pending {
                    std::fs::remove_file(pending).ok();
                }
                result
            }
            ExecutionState::ReadyToSignReveal(_) => unreachable!("execute builds the commit first"),
        }
    };

    confirm_regtest(provider, config).await?;

    Ok(result)
}

fn commit_txid(psbt: &bitcoin::psbt::Psbt) -> Result<String> {
    let stable = psbt.inputs.iter().all(|input| {
        input
            .witness_utxo
            .as_ref()
            .is_some_and(|utxo| utxo.script_pubkey.is_p2tr() || utxo.script_pubkey.is_p2wpkh())
    });
    if !stable {
        return Err(LabcoatError::new(
            "SIGNER_UNSUPPORTED",
            "cannot journal a commit with inputs whose signatures change its transaction id",
            "fund the project wallet with native SegWit inputs",
        ));
    }
    Ok(psbt.unsigned_tx.compute_txid().to_string())
}

/// Record the signed ID before submitting. Unlike an RPC response this
/// survives a lost connection after the node accepted the transaction.
async fn broadcast_signed(
    provider: &mut ConcreteProvider,
    psbt: bitcoin::psbt::Psbt,
    fee: u64,
    commit: Option<(String, u64)>,
    observe: &mut BroadcastObserver<'_>,
) -> Result<EnhancedExecuteResult> {
    let signed = provider
        .sign_psbt(&psbt)
        .await
        .map_err(|error| LabcoatError::classify(error.into()))?;
    let mut tx = signed
        .clone()
        .extract_tx()
        .map_err(|error| LabcoatError::classify(error.into()))?;
    for (input, psbt_input) in tx.input.iter_mut().zip(signed.inputs) {
        // Upstream keystore taproot key-path signatures are not finalized
        // into a PSBT witness; extract_tx already handles finalized inputs.
        if let Some(signature) = &psbt_input.tap_key_sig {
            input.witness = bitcoin::Witness::p2tr_key_spend(signature);
        }
    }
    let txid = tx.compute_txid().to_string();
    let tx_hex = bitcoin::consensus::encode::serialize_hex(&tx);
    observe(Broadcast::Execute(&txid))?;
    let received = provider
        .broadcast_transaction(tx_hex.clone())
        .await
        .map_err(|error| LabcoatError::classify(error.into()))?;
    if received != txid {
        return Err(LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("node returned transaction id {received} for {txid}"),
            "check the RPC endpoint and trace the recorded transaction id",
        ));
    }
    let (commit_txid, commit_fee) = commit.unzip();
    Ok(EnhancedExecuteResult {
        commit_txid,
        commit_fee,
        reveal_txid: txid,
        reveal_fee: fee,
        inputs_used: tx
            .input
            .iter()
            .map(|input| input.previous_output.to_string())
            .collect(),
        outputs_created: tx
            .output
            .iter()
            .map(|output| output.script_pubkey.to_string())
            .collect(),
        traces: None,
        split_txid: None,
        split_fee: None,
        wrap_txid: None,
        wrap_fee: None,
        reveal_tx_hex: Some(tx_hex),
    })
}

async fn confirm_regtest(provider: &ConcreteProvider, config: &ToolkitConfig) -> Result<()> {
    if config.network.uses_regtest() {
        provider
            .generate_to_address(1, &regtest_mining_address())
            .await
            .map_err(|error| LabcoatError::classify(error.into()))?;
        crate::sync::wait_for_indexer(provider, POST_BROADCAST_SYNC_TIMEOUT).await?;
    }
    Ok(())
}

fn approve_reveal(
    config: &ToolkitConfig,
    wallet_fingerprint: &Option<String>,
    reveal: &ReadyToSignRevealTx,
) -> Result<()> {
    approve_transaction(
        config,
        wallet_fingerprint,
        &preview(
            &format!("reveal (commit {})", reveal.commit_txid),
            &reveal.psbt,
            reveal.fee,
            config,
            wallet_fingerprint,
        ),
    )
}

pub(crate) async fn resume_reveal(
    provider: &mut ConcreteProvider,
    config: &ToolkitConfig,
    reveal: ReadyToSignRevealTx,
    observe: &mut BroadcastObserver<'_>,
) -> Result<EnhancedExecuteResult> {
    let fingerprint = provider
        .get_keystore()
        .ok()
        .map(|keystore| keystore.master_fingerprint.clone());
    approve_reveal(config, &fingerprint, &reveal)?;
    let path = pending_reveal_path(config, &reveal.commit_txid);
    let result = broadcast_signed(
        provider,
        reveal.psbt,
        reveal.fee,
        Some((reveal.commit_txid, reveal.commit_fee)),
        observe,
    )
    .await?;
    std::fs::remove_file(path).ok();
    confirm_regtest(provider, config).await?;
    Ok(result)
}

/// Mirrors the constant baked into upstream build_reveal_psbt
/// (alkanes-cli-common alkanes/execute.rs, `total_bitcoin_needed`).
const UPSTREAM_REVEAL_BUFFER: u64 = 50_000;
const DUST: u64 = 546;
/// Room for the commit transaction's own fee and change output on top of
/// what the reveal needs.
const COMMIT_HEADROOM: u64 = 20_000;

/// Coin control for envelope (commit/reveal) transactions.
///
/// Upstream sizes the commit's funding for the commit alone, oldest UTXO
/// first. In a wallet holding several small UTXOs — the change outputs a
/// multi-step deployment leaves behind — that selection can cover the
/// commit with almost no change, leaving [`prefund_reveal_from_commit_change`]
/// nothing to pre-fund the reveal from; the reveal then re-selects from a
/// stale view and is rejected as a conflicting replacement.
///
/// So choose the funding here instead: the largest plain UTXOs that
/// together cover `needed`, and exclude every other one. Returns nothing
/// to exclude when the wallet cannot cover `needed` at all, leaving the
/// upstream insufficient-funds path to report it.
fn envelope_funding_exclusions(spendable: &[(String, u64)], needed: u64) -> Vec<String> {
    let mut by_size: Vec<&(String, u64)> = spendable.iter().collect();
    by_size.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut total = 0_u64;
    let mut keep = 0;
    for (_, sats) in &by_size {
        if total >= needed {
            break;
        }
        total = total.saturating_add(*sats);
        keep += 1;
    }
    if total < needed {
        return Vec::new();
    }
    by_size[keep..]
        .iter()
        .map(|(outpoint, _)| outpoint.clone())
        .collect()
}

/// Upstream's `build_reveal_psbt` funds the reveal with extra wallet UTXOs
/// whenever the commit output holds less than `to·546 + bitcoin reqs +
/// 50_000` sats — and on this stack that selection reads a stale confirmed
/// view (the mempool-aware adjustment is skipped in qubitcoin mode), so it
/// re-picks the very UTXO the still-unconfirmed commit just spent and the
/// reveal is rejected as a conflicting replacement.
///
/// The commit PSBT is still unsigned here, so rebalance the shortfall from
/// the commit's change output into the commit output instead: the reveal
/// then spends only the commit outpoint and never selects again. The extra
/// sats come straight back through the reveal's own change output.
fn prefund_reveal_from_commit_change(
    commit: &mut alkanes_cli_common::alkanes::types::ReadyToSignCommitTx,
) {
    let bitcoin_requirements: u64 = commit
        .params
        .input_requirements
        .iter()
        .filter_map(|requirement| match requirement {
            InputRequirement::Bitcoin { amount } => Some(*amount),
            _ => None,
        })
        .sum();
    let reveal_needs = commit.params.to_addresses.len() as u64 * DUST
        + bitcoin_requirements
        + UPSTREAM_REVEAL_BUFFER;
    if commit.required_reveal_amount >= reveal_needs {
        return;
    }
    let bump = reveal_needs - commit.required_reveal_amount;

    // Output 0 is the envelope commit output (resume_commit_execution spends
    // `vout: 0`); the change, when present, is the last non-commit output
    // paying back to the wallet.
    let outputs = &mut commit.psbt.unsigned_tx.output;
    if outputs.len() < 2 {
        tracing::warn!(
            "commit transaction has no change output to pre-fund the reveal from; falling back to upstream reveal funding"
        );
        return;
    }
    let change_index = outputs.len() - 1;
    let change_value = outputs[change_index].value.to_sat();
    if change_value < bump + DUST {
        tracing::warn!(
            "commit change ({change_value} sats) cannot cover the reveal shortfall ({bump} sats); falling back to upstream reveal funding"
        );
        return;
    }
    outputs[0].value = bitcoin::Amount::from_sat(outputs[0].value.to_sat() + bump);
    outputs[change_index].value = bitcoin::Amount::from_sat(change_value - bump);
    commit.required_reveal_amount = reveal_needs;
    tracing::debug!("pre-funded the reveal with {bump} extra sats through the commit output");
}

/// Render a short human preview of a transaction about to be signed.
fn preview(
    kind: &str,
    psbt: &bitcoin::psbt::Psbt,
    fee: u64,
    config: &ToolkitConfig,
    wallet_fingerprint: &Option<String>,
) -> String {
    let network = match config.bitcoin_network_id() {
        "mainnet" => bitcoin::Network::Bitcoin,
        "testnet" => bitcoin::Network::Testnet,
        "signet" => bitcoin::Network::Signet,
        _ => bitcoin::Network::Regtest,
    };
    let mut lines = vec![format!(
        "about to sign {kind} on {} (wallet {})",
        config.network_id(),
        wallet_fingerprint.as_deref().unwrap_or("unknown"),
    )];
    let tx = &psbt.unsigned_tx;
    lines.push(format!(
        "  {} input(s), {} output(s), fee {fee} sats",
        tx.input.len(),
        tx.output.len()
    ));
    for (index, output) in tx.output.iter().enumerate() {
        let destination = if output.script_pubkey.is_op_return() {
            "OP_RETURN (protostone)".to_string()
        } else {
            bitcoin::Address::from_script(&output.script_pubkey, network)
                .map(|address| address.to_string())
                .unwrap_or_else(|_| "non-standard script".to_string())
        };
        lines.push(format!(
            "  out v{index}: {destination} {} sats",
            output.value.to_sat()
        ));
    }
    lines.join("\n")
}

/// The approval gate for every signature. Regtest targets (and `--yes`)
/// auto-approve after printing the preview; public networks require an
/// interactive confirmation.
fn approve_transaction(
    config: &ToolkitConfig,
    _wallet_fingerprint: &Option<String>,
    preview: &str,
) -> Result<()> {
    eprintln!("{preview}");
    if config.network.uses_regtest() || config.assume_yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        return Err(LabcoatError::new(
            "APPROVAL_REQUIRED",
            format!(
                "signing on {} requires interactive approval",
                config.network_id()
            ),
            "re-run in a terminal, or pass --yes to approve non-interactively",
        ));
    }
    eprint!("sign and broadcast? [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).map_err(|error| {
        LabcoatError::new(
            "APPROVAL_REQUIRED",
            format!("could not read approval: {error}"),
            "re-run in a terminal, or pass --yes to approve non-interactively",
        )
    })?;
    let answer = answer.trim().to_ascii_lowercase();
    if answer == "y" || answer == "yes" {
        Ok(())
    } else {
        Err(LabcoatError::new(
            "APPROVAL_DECLINED",
            "transaction was not approved",
            "nothing was signed or broadcast",
        ))
    }
}

fn pending_reveal_path(config: &ToolkitConfig, commit_txid: &str) -> std::path::PathBuf {
    let dir = config
        .wallet_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join("pending"))
        .unwrap_or_else(|| std::path::PathBuf::from(".labcoat/pending"));
    dir.join(format!("reveal-{commit_txid}.json"))
}

pub(crate) fn load_pending_reveal(
    config: &ToolkitConfig,
    commit_txid: &str,
) -> Option<ReadyToSignRevealTx> {
    let txid = bitcoin::Txid::from_str(commit_txid).ok()?;
    let bytes = std::fs::read(pending_reveal_path(config, &txid.to_string())).ok()?;
    let reveal: ReadyToSignRevealTx = serde_json::from_slice(&bytes).ok()?;
    let input = reveal.psbt.unsigned_tx.input.first()?;
    (reveal.commit_txid == commit_txid && input.previous_output == bitcoin::OutPoint::new(txid, 0))
        .then_some(reveal)
}

/// Best-effort persistence of a ready-to-sign reveal: the commit is already
/// on chain or in the mempool, so a local write failure cannot roll it back.
fn persist_pending_reveal(
    config: &ToolkitConfig,
    reveal: &ReadyToSignRevealTx,
) -> Option<std::path::PathBuf> {
    let path = pending_reveal_path(config, &reveal.commit_txid);
    let dir = path.parent().expect("pending reveal directory");
    if let Err(error) = std::fs::create_dir_all(dir) {
        tracing::warn!("cannot create {}: {error}", dir.display());
        return None;
    }
    match serde_json::to_vec_pretty(reveal) {
        Ok(bytes) => {
            if let Err(error) = std::fs::write(&path, bytes) {
                tracing::warn!("cannot persist pending reveal {}: {error}", path.display());
                return None;
            }
            Some(path)
        }
        Err(error) => {
            tracing::warn!("cannot serialize pending reveal: {error}");
            None
        }
    }
}

/// Qubitcoin's Esplora transaction shape can omit the coinbase marker. Confirm
/// recent wallet outputs against gettxout so retries never select an immature
/// block reward, even if the secondary index misclassified it.
///
/// Every UTXO the alkanes indexer reports as carrying tokens is protected
/// from plain bitcoin funding — spending one without accounting for its
/// assets burns or misroutes them — EXCEPT outpoints holding an alkane this
/// transaction explicitly requires, which the selector must remain free to
/// pick. The wallet's own `has_alkanes` flag is not populated on this
/// stack, so the outpoints come from `protorunes_by_address` against the
/// indexer.
async fn protected_outpoints(
    provider: &ConcreteProvider,
    mine_enabled: bool,
    required_alkanes: &std::collections::BTreeSet<(u128, u128)>,
) -> Result<(Vec<String>, Vec<(String, u64)>)> {
    use alkanes_cli_common::traits::AlkanesProvider;

    let utxos = WalletProvider::get_utxos(provider, true, None)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    let mut excluded = Vec::new();

    let mut addresses: Vec<String> = utxos.iter().map(|(_, i)| i.address.clone()).collect();
    addresses.sort_unstable();
    addresses.dedup();
    for address in addresses {
        let holdings = AlkanesProvider::protorunes_by_address(provider, &address, None, 1)
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
        for entry in holdings.balances {
            let holds_required = entry
                .balance_sheet
                .cached
                .balances
                .iter()
                .any(|(id, amount)| *amount > 0 && required_alkanes.contains(&(id.block, id.tx)));
            if !holds_required {
                excluded.push(entry.outpoint.to_string());
            }
        }
    }

    for (outpoint, info) in &utxos {
        if !mine_enabled || info.confirmations >= 100 {
            continue;
        }
        let txout = BitcoinRpcProvider::get_tx_out(
            provider,
            &outpoint.txid.to_string(),
            outpoint.vout,
            true,
        )
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
        if txout.is_null()
            || info.is_coinbase
            || txout.get("coinbase").and_then(|v| v.as_bool()) == Some(true)
        {
            excluded.push(outpoint.to_string());
        }
    }

    // Plain bitcoin the selector may still fund with.
    let spendable = utxos
        .iter()
        .map(|(outpoint, info)| (outpoint.to_string(), info.amount))
        .filter(|(outpoint, _)| !excluded.contains(outpoint))
        .collect();
    Ok((excluded, spendable))
}

/// Mine confirmations away from the project wallet. The public key is the
/// secp256k1 generator point; this is only a regtest block-reward sink.
pub(crate) fn regtest_mining_address() -> String {
    let public_key = bitcoin::secp256k1::PublicKey::from_str(
        "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
    )
    .expect("fixed compressed public key");
    bitcoin::Address::p2wpkh(
        &bitcoin::CompressedPublicKey(public_key),
        bitcoin::Network::Regtest,
    )
    .to_string()
}

/// Extract `block:tx` of a newly created alkane from trace events.
pub fn find_created_alkane(traces: &Option<Vec<serde_json::Value>>) -> Option<String> {
    let traces = traces.as_ref()?;
    for trace in traces {
        if let Some(id) = scan_for_create(trace) {
            return Some(id);
        }
    }
    None
}

/// Every Alkane id created in the traces, in trace order without
/// duplicates. A deploy's own contract comes first; contracts it creates
/// while initializing (an auth token, for example) follow.
pub fn find_created_alkanes(traces: &Option<Vec<serde_json::Value>>) -> Vec<String> {
    fn scan(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                let is_create = map.get("event").and_then(|e| e.as_str()) == Some("create")
                    || map.get("type").and_then(|t| t.as_str()) == Some("create_alkane");
                if is_create {
                    let id = ["data", "alkane_id", "new_alkane"]
                        .iter()
                        .find_map(|key| map.get(*key).and_then(extract_id));
                    if let Some(id) = id {
                        if !out.contains(&id) {
                            out.push(id);
                        }
                        return;
                    }
                }
                map.values().for_each(|inner| scan(inner, out));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|inner| scan(inner, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(traces) = traces {
        traces.iter().for_each(|trace| scan(trace, &mut out));
    }
    out
}

fn scan_for_create(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            // Shapes seen from trace_to_json: {"event":"create","data":{"block":N,"tx":M}}
            // and {"type":"create_alkane","alkane_id"/"new_alkane":{"block":..,"tx":..}}
            let is_create = map
                .get("event")
                .and_then(|e| e.as_str())
                .map(|e| e == "create")
                .unwrap_or(false)
                || map
                    .get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| t == "create_alkane")
                    .unwrap_or(false);
            if is_create {
                for key in ["data", "alkane_id", "new_alkane"] {
                    if let Some(idv) = map.get(key) {
                        if let Some(id) = extract_id(idv) {
                            return Some(id);
                        }
                    }
                }
            }
            for v in map.values() {
                if let Some(found) = scan_for_create(v) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => items.iter().find_map(scan_for_create),
        _ => None,
    }
}

fn extract_id(value: &serde_json::Value) -> Option<String> {
    let block = value.get("block")?;
    let tx = value.get("tx")?;
    let to_num = |v: &serde_json::Value| -> Option<u128> {
        if let Some(n) = v.as_u64() {
            return Some(n as u128);
        }
        let s = v.as_str()?;
        if let Some(hex) = s.strip_prefix("0x") {
            u128::from_str_radix(hex, 16).ok()
        } else {
            s.parse().ok()
        }
    };
    Some(format!("{}:{}", to_num(block)?, to_num(tx)?))
}

/// Extract the return status ("success" | "revert" | "unknown") and any
/// decoded revert reason from trace events.
pub fn find_return_status(traces: &Option<Vec<serde_json::Value>>) -> (String, Option<String>) {
    let Some(traces) = traces else {
        return ("unknown".to_string(), None);
    };
    for trace in traces {
        if let Some(found) = scan_for_return(trace) {
            return found;
        }
    }
    ("unknown".to_string(), None)
}

fn scan_for_return(value: &serde_json::Value) -> Option<(String, Option<String>)> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(events) = map
                .get("trace")
                .or_else(|| map.get("events"))
                .and_then(|events| events.as_array())
            {
                return scan_execution_events(events);
            }
            let event = map.get("event").and_then(|e| e.as_str());
            let typ = map.get("type").and_then(|t| t.as_str());
            if matches!(event, Some("return" | "revert"))
                || matches!(typ, Some("return" | "revert"))
            {
                let data = map.get("data");
                let status = data
                    .and_then(|d| d.get("status"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        if typ == Some("revert") || event == Some("revert") {
                            "revert".to_string()
                        } else {
                            "success".to_string()
                        }
                    });
                let reason = map
                    .get("error_message")
                    .or_else(|| data.and_then(|d| d.get("error_message")))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        data.and_then(|d| d.get("response"))
                            .and_then(|r| r.get("data"))
                            .and_then(|d| d.as_str())
                            .and_then(decode_revert_reason)
                    });
                return Some((status, reason));
            }
            map.values().find_map(scan_for_return)
        }
        serde_json::Value::Array(items) => scan_execution_events(items),
        _ => None,
    }
}

fn scan_execution_events(events: &[serde_json::Value]) -> Option<(String, Option<String>)> {
    let mut depth = 0usize;
    let mut status = None;
    for event in events {
        let kind = event
            .get("type")
            .or_else(|| event.get("event"))
            .and_then(|kind| kind.as_str());
        match kind {
            Some("call" | "delegatecall" | "staticcall") => depth += 1,
            Some("return" | "revert") => {
                if depth <= 1 {
                    status = scan_for_return(event);
                }
                depth = depth.saturating_sub(1);
            }
            _ if depth == 0 => {
                if let Some(result) = scan_for_return(event) {
                    status = Some(result);
                }
            }
            _ => {}
        }
    }
    status
}

/// Skip the "0x" prefix and 4-byte selector, then interpret the rest as
/// UTF-8.
pub fn decode_revert_reason(hex_str: &str) -> Option<String> {
    if hex_str.is_empty() || hex_str == "0x" {
        return None;
    }
    let data = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    if data.len() <= 8 {
        return None;
    }
    let bytes = hex::decode(&data[8..]).ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_return_does_not_hide_a_root_revert() {
        let traces = Some(vec![serde_json::json!({"trace": [
            {"type": "call"},
            {"type": "delegatecall"},
            {"type": "return"},
            {"type": "revert", "error_message": "outer call failed"}
        ]})]);
        assert_eq!(
            find_return_status(&traces),
            ("revert".into(), Some("outer call failed".into()))
        );
    }

    #[test]
    fn serde_json_preserves_full_u128_cellpack_values() {
        let value = serde_json::json!(u128::MAX);
        assert_eq!(value.to_string(), u128::MAX.to_string());
    }

    #[test]
    fn alkane_inputs_prepend_a_splitter_protostone() {
        // Exact required amounts route to the call (p1); excess returns to
        // output 0. Amount 0 (ALL) passes through as a route-everything edict.
        let options = TxOptions {
            inputs: Some("4:65014:100,4:65012:7500".into()),
            ..TxOptions::default()
        };
        assert_eq!(
            spec_with_options(4, 65_014, &[10], &options),
            "v0:v0:[4:65014:100:p1]:[4:65012:7500:p1],[4,65014,10]:v0:v0"
        );

        let all = TxOptions {
            inputs: Some("4:65014:0".into()),
            ..TxOptions::default()
        };
        assert_eq!(
            spec_with_options(4, 65_014, &[1], &all),
            "v0:v0:[4:65014:0:p1],[4,65014,1]:v0:v0"
        );
    }

    #[test]
    fn splitter_shifts_caller_protostone_references() {
        // pN pointers/refunds/edict targets shift one slot for the prepended
        // splitter; vN output references are untouched.
        let options = TxOptions {
            inputs: Some("4:65014:100".into()),
            pointer: Some("p0".into()),
            refund: Some("v1".into()),
            edicts: vec!["4:65014:100:p0".into(), "4:65014:5:v2".into()],
            ..TxOptions::default()
        };
        assert_eq!(
            spec_with_options(4, 65_014, &[10], &options),
            "v0:v0:[4:65014:100:p1],[4,65014,10]:p1:v1:[4:65014:100:p1]:[4:65014:5:v2]"
        );
    }

    #[test]
    fn bitcoin_only_inputs_do_not_split() {
        let options = TxOptions {
            inputs: Some("B:10000".into()),
            ..TxOptions::default()
        };
        assert_eq!(
            spec_with_options(4, 65_014, &[10], &options),
            "[4,65014,10]:v0:v0"
        );
    }

    #[test]
    fn default_options_reproduce_the_historical_spec_shape() {
        assert_eq!(cellpack_spec(2, 1, 20, &[]), "[2,1,20]:v0:v0");
        assert_eq!(cellpack_spec(4, 65_010, 10, &[7]), "[4,65010,10,7]:v0:v0");
        assert_eq!(
            spec_with_options(1, 0, &[], &TxOptions::default()),
            "[1,0]:v0:v0"
        );
    }

    #[test]
    fn routing_and_edicts_render_and_reparse() {
        let options = TxOptions {
            pointer: Some("v1".into()),
            refund: None,
            edicts: vec!["4:65014:100:v0".into(), "[4:65014:5:v1]".into()],
            ..TxOptions::default()
        };
        let spec = spec_with_options(4, 65_014, &[101], &options);
        // Refund defaults to the pointer; pre-bracketed edicts are normalized.
        assert_eq!(spec, "[4,65014,101]:v1:v1:[4:65014:100:v0]:[4:65014:5:v1]");
        validate_spec(&spec).expect("upstream grammar accepts the built spec");
    }

    #[test]
    fn bad_routing_targets_fail_upstream_validation() {
        let options = TxOptions {
            pointer: Some("x9".into()),
            ..TxOptions::default()
        };
        let spec = spec_with_options(2, 1, &[20], &options);
        let err = validate_spec(&spec).expect_err("x9 is not a vN/pN target");
        assert_eq!(err.code, "ENVELOPE_INVALID");
    }

    #[test]
    fn envelope_funding_keeps_the_largest_utxos_and_excludes_the_rest() {
        let spendable = vec![
            ("small:0".to_string(), 546),
            ("change:1".to_string(), 19_871),
            ("big:1".to_string(), 499_838_860),
            ("change:2".to_string(), 17_707),
        ];
        // One large UTXO covers the need: every small one is excluded.
        assert_eq!(
            envelope_funding_exclusions(&spendable, 70_546),
            ["change:1", "change:2", "small:0"]
        );
        // The need spans several UTXOs, largest first.
        let fragmented = vec![
            ("a:0".to_string(), 40_000),
            ("b:0".to_string(), 30_000),
            ("c:0".to_string(), 20_000),
            ("d:0".to_string(), 546),
        ];
        assert_eq!(envelope_funding_exclusions(&fragmented, 70_546), ["d:0"]);
        // A wallet that cannot cover the need is left to the upstream
        // insufficient-funds error.
        assert!(envelope_funding_exclusions(&fragmented[2..], 70_546).is_empty());
    }

    #[test]
    fn input_requirements_parse_alkanes_and_bitcoin_entries() {
        let options = TxOptions {
            inputs: Some("4:65011:100,B:5000".into()),
            ..TxOptions::default()
        };
        let requirements = options.input_requirements().unwrap();
        assert_eq!(requirements.len(), 2);
        assert!(matches!(
            requirements[0],
            InputRequirement::Alkanes {
                block: 4,
                tx: 65_011,
                amount: 100
            }
        ));
        assert!(matches!(
            requirements[1],
            InputRequirement::Bitcoin { amount: 5000 }
        ));

        let bad = TxOptions {
            inputs: Some("4:65011".into()),
            ..TxOptions::default()
        };
        assert_eq!(bad.input_requirements().unwrap_err().code, "CONFIG_INVALID");
        assert!(TxOptions::default()
            .input_requirements()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn regtest_confirmation_mining_uses_a_valid_non_wallet_address() {
        let address = bitcoin::Address::from_str(&regtest_mining_address())
            .unwrap()
            .require_network(bitcoin::Network::Regtest)
            .unwrap();
        assert_eq!(address.to_string(), regtest_mining_address());
    }
}
