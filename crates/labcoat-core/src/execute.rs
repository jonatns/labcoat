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
    let excluded_utxos = protected_outpoints(provider, mine_enabled, &required_alkanes).await?;

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
                executor
                    .resume_execution(tx, &params)
                    .await
                    .map_err(|e| LabcoatError::classify(e.into()))?
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
                let next = executor
                    .resume_commit_execution(commit)
                    .await
                    .map_err(|e| LabcoatError::classify(e.into()))?;
                let reveal = match next {
                    ExecutionState::ReadyToSignReveal(reveal) => reveal,
                    other => {
                        return Err(LabcoatError::new(
                            "TOOLKIT_ERROR",
                            format!("unexpected state after commit broadcast: {other:?}"),
                            "report this as a Labcoat bug",
                        ));
                    }
                };
                // The commit is on the network from here on — persist the
                // reveal state first so a crash or a declined approval can
                // be recovered without abandoning the commit output.
                let pending = persist_pending_reveal(config, &reveal);
                approve_transaction(
                    config,
                    &wallet_fingerprint,
                    &preview(
                        &format!("reveal (commit {})", reveal.commit_txid),
                        &reveal.psbt,
                        reveal.fee,
                        config,
                        &wallet_fingerprint,
                    ),
                )?;
                let result = executor
                    .resume_reveal_execution(reveal)
                    .await
                    .map_err(|e| LabcoatError::classify(e.into()))?;
                if let Some(pending) = pending {
                    std::fs::remove_file(pending).ok();
                }
                result
            }
            ExecutionState::ReadyToSignReveal(reveal) => {
                // execute() never starts here, but resuming a persisted
                // reveal will once `labcoat resume` lands.
                let result = executor
                    .resume_reveal_execution(reveal)
                    .await
                    .map_err(|e| LabcoatError::classify(e.into()))?;
                result
            }
        }
    };

    if mine_enabled {
        provider
            .generate_to_address(1, &regtest_mining_address())
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
        crate::sync::wait_for_indexer(provider, POST_BROADCAST_SYNC_TIMEOUT).await?;
    }

    Ok(result)
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
    // Mirrors the constant baked into upstream build_reveal_psbt
    // (alkanes-cli-common alkanes/execute.rs, `total_bitcoin_needed`).
    const UPSTREAM_REVEAL_BUFFER: u64 = 50_000;
    const DUST: u64 = 546;

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

/// Best-effort persistence of a ready-to-sign reveal so an interrupted
/// deploy can be recovered; the commit is already broadcast when this runs,
/// so failing the whole flow over a persistence error would only make
/// recovery harder.
fn persist_pending_reveal(
    config: &ToolkitConfig,
    reveal: &ReadyToSignRevealTx,
) -> Option<std::path::PathBuf> {
    let dir = config
        .wallet_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.join("pending"))
        .unwrap_or_else(|| std::path::PathBuf::from(".labcoat/pending"));
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!("cannot create {}: {error}", dir.display());
        return None;
    }
    let path = dir.join(format!("reveal-{}.json", reveal.commit_txid));
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
) -> Result<Vec<String>> {
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

    for (outpoint, info) in utxos {
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

    Ok(excluded)
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
            let event = map.get("event").and_then(|e| e.as_str());
            let typ = map.get("type").and_then(|t| t.as_str());
            if event == Some("return") || typ == Some("return") || typ == Some("revert") {
                let data = map.get("data");
                let status = data
                    .and_then(|d| d.get("status"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        if typ == Some("revert") {
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
        serde_json::Value::Array(items) => items.iter().find_map(scan_for_return),
        _ => None,
    }
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
