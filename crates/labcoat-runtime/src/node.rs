//! Labcoat Network's JSON-RPC interface.
//!
//! The methods and their results match the subset of Bitcoin Core and
//! Qubitcoin that Labcoat and the Alkanes client library use, so either can
//! talk to Labcoat Network without knowing what serves it:
//!
//! - chain: `getblockcount`, `getbestblockhash`, `getblockhash`, `getblock`,
//!   `getblockheader`, `getblockchaininfo`, `getnetworkinfo`;
//! - transactions: `getrawtransaction`, `decoderawtransaction`, `gettxout`,
//!   `sendrawtransaction`, `testmempoolaccept`, `getmempoolinfo`,
//!   `getrawmempool`;
//! - mining: `generatetoaddress`;
//! - indexers: `secondaryheight` and `secondaryview` for `alkanes` and
//!   `esplora`, and `metashrew_height` and `metashrew_view`;
//! - development: `labcoat_snapshot`, `labcoat_revert`, `labcoat_info`, and
//!   `stop`.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Instant;

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hex::{DisplayHex, FromHex};
use bitcoin::{Address, Amount, Block, BlockHash, Network, OutPoint, Script, Transaction, Txid};
use serde_json::{json, Value};

use crate::{esplora, Runtime, RuntimeError, Snapshot};

/// The most blocks one `generatetoaddress` call mines.
const MAX_GENERATE: u64 = 1_000;
/// Bitcoin Core's default `maxfeerate` for `sendrawtransaction`, in BTC/kvB.
const DEFAULT_MAX_FEE_RATE: f64 = 0.10;
const ALKANES: &str = "alkanes";
const ESPLORA: &str = "esplora";

/// A JSON-RPC error: Bitcoin Core's code and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn params(message: impl Into<String>) -> Self {
        Self::new(-8, message)
    }

    fn misc(message: impl Into<String>) -> Self {
        Self::new(-1, message)
    }
}

impl From<RuntimeError> for RpcError {
    fn from(error: RuntimeError) -> Self {
        match error {
            RuntimeError::Rejected(rejection) => Self::new(rejection.code, rejection.reason),
            other => Self::misc(other.to_string()),
        }
    }
}

type Result<T> = std::result::Result<T, RpcError>;

/// A runtime served over JSON-RPC.
pub struct Node {
    runtime: RwLock<Runtime>,
    snapshots: Mutex<Vec<Snapshot>>,
    stopping: AtomicBool,
    started: Instant,
    data_dir: Option<String>,
}

impl Node {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime: RwLock::new(runtime),
            snapshots: Mutex::new(Vec::new()),
            stopping: AtomicBool::new(false),
            started: Instant::now(),
            data_dir: None,
        }
    }

    /// Name the directory the chain is saved in, for `labcoat_info`.
    pub fn with_data_dir(mut self, data_dir: impl Into<String>) -> Self {
        self.data_dir = Some(data_dir.into());
        self
    }

    /// Whether a `stop` request arrived.
    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// Answer an HTTP request body: one JSON-RPC request or a batch.
    pub fn handle_body(&self, body: &[u8]) -> Vec<u8> {
        let response = match serde_json::from_slice::<Value>(body) {
            Ok(Value::Array(requests)) => Value::from(
                requests
                    .iter()
                    .map(|request| self.handle_request(request))
                    .collect::<Vec<_>>(),
            ),
            Ok(request) => self.handle_request(&request),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "error": { "code": -32700, "message": format!("Parse error: {error}") },
                "id": null,
            }),
        };
        response.to_string().into_bytes()
    }

    /// Answer one JSON-RPC request object.
    pub fn handle_request(&self, request: &Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return json!({
                "jsonrpc": "2.0",
                "error": { "code": -32600, "message": "Invalid Request" },
                "id": id,
            });
        };
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        match self.call(method, &params) {
            Ok(result) => json!({ "jsonrpc": "2.0", "result": result, "id": id }),
            Err(error) => {
                tracing::debug!(method, code = error.code, "{}", error.message);
                json!({
                    "jsonrpc": "2.0",
                    "error": { "code": error.code, "message": error.message },
                    "id": id,
                })
            }
        }
    }

    /// Run one method.
    pub fn call(&self, method: &str, params: &Value) -> Result<Value> {
        match method {
            "getblockcount" => Ok(self.read().height().into()),
            "getbestblockhash" => Ok(self.read().chain().tip().block_hash().to_string().into()),
            "getblockhash" => {
                let height = u32_param(params, 0, "height")?;
                self.read()
                    .chain()
                    .block_hash(height)
                    .map(|hash| hash.to_string().into())
                    .ok_or_else(|| RpcError::params("Block height out of range"))
            }
            "getblock" => {
                let runtime = self.read();
                let (height, block) = block_param(&runtime, params)?;
                match verbosity(param(params, 1), 1) {
                    0 => Ok(serialize(block).to_lower_hex_string().into()),
                    level => Ok(block_json(&runtime, height, block, level >= 2)),
                }
            }
            "getblockheader" => {
                let runtime = self.read();
                let (height, block) = block_param(&runtime, params)?;
                if verbosity(param(params, 1), 1) == 0 {
                    return Ok(serialize(&block.header).to_lower_hex_string().into());
                }
                let mut header = block_json(&runtime, height, block, false);
                if let Value::Object(fields) = &mut header {
                    fields.remove("tx");
                }
                Ok(header)
            }
            "getblockchaininfo" => {
                let runtime = self.read();
                let tip = runtime.chain().tip();
                Ok(json!({
                    "chain": "regtest",
                    "blocks": runtime.height(),
                    "headers": runtime.height(),
                    "bestblockhash": tip.block_hash().to_string(),
                    "difficulty": difficulty(),
                    "time": tip.header.time,
                    "mediantime": median_time(&runtime),
                    "verificationprogress": 1,
                    "initialblockdownload": false,
                    "chainwork": format!("{:064x}", u64::from(runtime.height()) * 2 + 2),
                    "size_on_disk": 0,
                    "pruned": false,
                    "warnings": "",
                }))
            }
            "getnetworkinfo" => Ok(json!({
                "version": 300000,
                "subversion": format!("/LabcoatNetwork:{}/", env!("CARGO_PKG_VERSION")),
                "protocolversion": 70016,
                "connections": 0,
                "networkactive": false,
                "relayfee": 0.00001,
                "incrementalfee": 0.00001,
                "warnings": "",
            })),
            "getmempoolinfo" => {
                let runtime = self.read();
                let mempool = runtime.mempool();
                let bytes: u64 = mempool.entries().iter().map(|entry| entry.vsize()).sum();
                Ok(json!({
                    "loaded": true,
                    "size": mempool.len(),
                    "bytes": bytes,
                    "usage": bytes,
                    "total_fee": mempool.total_fee().to_btc(),
                    "maxmempool": 300_000_000,
                    "mempoolminfee": 0.00001,
                    "minrelaytxfee": 0.00001,
                    "incrementalrelayfee": 0.00001,
                    "unbroadcastcount": 0,
                    "fullrbf": true,
                }))
            }
            "getrawmempool" => {
                let runtime = self.read();
                let entries = runtime.mempool().entries();
                if verbosity(param(params, 0), 0) == 0 {
                    return Ok(Value::from(
                        entries
                            .iter()
                            .map(|entry| entry.txid.to_string())
                            .collect::<Vec<_>>(),
                    ));
                }
                Ok(Value::Object(
                    entries
                        .iter()
                        .map(|entry| {
                            (
                                entry.txid.to_string(),
                                json!({
                                    "vsize": entry.vsize(),
                                    "weight": entry.weight.to_wu(),
                                    "fees": { "base": entry.fee.to_btc() },
                                    "height": runtime.height(),
                                }),
                            )
                        })
                        .collect(),
                ))
            }
            "gettxout" => {
                let runtime = self.read();
                let txid = txid_param(params, 0)?;
                let outpoint = OutPoint::new(txid, u32_param(params, 1, "n")?);
                let include_mempool = param(params, 2).and_then(Value::as_bool).unwrap_or(true);
                if include_mempool && runtime.mempool().spender(&outpoint).is_some() {
                    return Ok(Value::Null);
                }
                let (output, confirmations, coinbase) = match runtime.ledger().coin(&outpoint) {
                    Some(coin) => (
                        &coin.output,
                        runtime.height() - coin.height + 1,
                        coin.coinbase,
                    ),
                    None => match runtime
                        .mempool()
                        .output(&outpoint)
                        .filter(|_| include_mempool)
                    {
                        Some(output) => (output, 0, false),
                        None => return Ok(Value::Null),
                    },
                };
                Ok(json!({
                    "bestblock": runtime.chain().tip().block_hash().to_string(),
                    "confirmations": confirmations,
                    "value": output.value.to_btc(),
                    "scriptPubKey": script_json(&output.script_pubkey),
                    "coinbase": coinbase,
                }))
            }
            "getrawtransaction" => {
                let runtime = self.read();
                let txid = txid_param(params, 0)?;
                let (tx, location) = esplora::find(&runtime, &txid.to_string()).ok_or_else(|| {
                    RpcError::new(
                        -5,
                        "No such mempool or blockchain transaction. Use gettransaction for wallet transactions.",
                    )
                })?;
                if verbosity(param(params, 1), 0) == 0 {
                    return Ok(serialize(tx).to_lower_hex_string().into());
                }
                let mut value = transaction_json(tx);
                if let Some(block) = location.and_then(|at| runtime.chain().block(at.height)) {
                    let height = location.expect("checked").height;
                    value["blockhash"] = block.block_hash().to_string().into();
                    value["confirmations"] = (runtime.height() - height + 1).into();
                    value["time"] = block.header.time.into();
                    value["blocktime"] = block.header.time.into();
                }
                Ok(value)
            }
            "decoderawtransaction" => Ok(transaction_json(&transaction_param(params)?)),
            "sendrawtransaction" => {
                let tx = transaction_param(params)?;
                let max = max_fee_rate(param(params, 1))?;
                let mut runtime = self.write();
                Ok(runtime
                    .submit(tx, max)
                    .map_err(|rejection| RpcError::new(rejection.code, rejection.reason))?
                    .to_string()
                    .into())
            }
            "testmempoolaccept" => {
                let raw = param(params, 0)
                    .and_then(Value::as_array)
                    .ok_or_else(|| RpcError::params("Expected an array of raw transactions"))?;
                let max = max_fee_rate(param(params, 1))?;
                let runtime = self.read();
                raw.iter()
                    .map(|hex| {
                        let tx = decode_transaction(hex.as_str().unwrap_or_default())?;
                        let mut result = json!({
                            "txid": tx.compute_txid().to_string(),
                            "wtxid": tx.compute_wtxid().to_string(),
                        });
                        match runtime.test(&tx, max) {
                            Ok(accepted) => {
                                result["allowed"] = true.into();
                                result["vsize"] = accepted.weight.to_vbytes_ceil().into();
                                result["fees"] = json!({ "base": accepted.fee.to_btc() });
                            }
                            Err(rejection) => {
                                result["allowed"] = false.into();
                                result["reject-reason"] = rejection.reason.into();
                            }
                        }
                        Ok(result)
                    })
                    .collect::<Result<Vec<_>>>()
                    .map(Value::from)
            }
            "generatetoaddress" => {
                let count = param(params, 0)
                    .and_then(Value::as_u64)
                    .ok_or_else(|| RpcError::params("Invalid nblocks"))?;
                if count > MAX_GENERATE {
                    return Err(RpcError::params(format!(
                        "Cannot mine more than {MAX_GENERATE} blocks at once"
                    )));
                }
                let address = param(params, 1)
                    .and_then(Value::as_str)
                    .and_then(|address| Address::from_str(address).ok())
                    .and_then(|address| address.require_network(Network::Regtest).ok())
                    .ok_or_else(|| RpcError::new(-5, "Error: Invalid address"))?;
                let mut runtime = self.write();
                let mut hashes = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let height = runtime.mine(address.script_pubkey())?;
                    hashes.push(
                        runtime
                            .chain()
                            .block_hash(height)
                            .expect("just mined")
                            .to_string(),
                    );
                }
                Ok(hashes.into())
            }
            "secondaryheight" => match str_param(params, 0)? {
                ALKANES => Ok(self.read().indexer().tip().unwrap_or(0).into()),
                ESPLORA => Ok(self.read().height().into()),
                other => Err(RpcError::misc(format!("indexer '{other}' not found"))),
            },
            "secondaryview" => {
                let label = str_param(params, 0)?;
                let view = str_param(params, 1)?;
                let input = hex_param(params, 2)?;
                let runtime = self.read();
                let output = match label {
                    ALKANES => runtime.view(view, &input)?,
                    ESPLORA => esplora::view(&runtime, view, &input).map_err(RpcError::misc)?,
                    other => return Err(RpcError::misc(format!("indexer '{other}' not found"))),
                };
                Ok(format!("0x{}", output.to_lower_hex_string()).into())
            }
            "metashrew_height" => Ok(self.read().indexer().tip().unwrap_or(0).into()),
            "metashrew_view" => {
                let view = str_param(params, 0)?;
                let input = hex_param(params, 1)?;
                let output = self.read().view(view, &input)?;
                Ok(format!("0x{}", output.to_lower_hex_string()).into())
            }
            // Both snapshot methods take the snapshot list first and the
            // runtime second, so a snapshot is registered before any revert
            // can run, and a revert discards every snapshot taken after it.
            "labcoat_snapshot" => {
                let mut snapshots = self.snapshots.lock().unwrap_or_else(|p| p.into_inner());
                let snapshot = self.read().snapshot();
                snapshots.push(snapshot);
                Ok((snapshots.len() - 1).into())
            }
            "labcoat_revert" => {
                let id = param(params, 0)
                    .and_then(Value::as_u64)
                    .ok_or_else(|| RpcError::params("Expected a snapshot id"))?
                    as usize;
                let mut snapshots = self.snapshots.lock().unwrap_or_else(|p| p.into_inner());
                if id >= snapshots.len() {
                    return Ok(false.into());
                }
                // Like evm_revert: reverting consumes this snapshot and every
                // later one.
                let snapshot = snapshots.drain(id..).next().expect("checked the id");
                self.write().revert(snapshot)?;
                Ok(true.into())
            }
            // Who is serving: Labcoat checks this before trusting a node
            // it finds on its port, which could be another version, another
            // data directory, or a different program.
            "labcoat_info" => Ok(json!({
                "name": "Labcoat Network",
                "version": env!("CARGO_PKG_VERSION"),
                "dataDir": self.data_dir,
                "height": self.read().height(),
            })),
            "uptime" => Ok(self.started.elapsed().as_secs().into()),
            "ping" => Ok(Value::Null),
            "stop" => {
                self.stopping.store(true, Ordering::SeqCst);
                Ok("Labcoat Network stopping".into())
            }
            _ => Err(RpcError::new(-32601, format!("Method not found: {method}"))),
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Runtime> {
        self.runtime
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Runtime> {
        self.runtime
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn param(params: &Value, index: usize) -> Option<&Value> {
    params
        .as_array()?
        .get(index)
        .filter(|value| !value.is_null())
}

fn str_param(params: &Value, index: usize) -> Result<&str> {
    param(params, index)
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::params(format!("Expected a string at position {index}")))
}

fn u32_param(params: &Value, index: usize, name: &str) -> Result<u32> {
    param(params, index)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| RpcError::params(format!("Invalid {name}")))
}

fn txid_param(params: &Value, index: usize) -> Result<Txid> {
    Txid::from_str(str_param(params, index)?)
        .map_err(|_| RpcError::params("txid must be of length 64 (not 0, for '')"))
}

fn hex_param(params: &Value, index: usize) -> Result<Vec<u8>> {
    let text = str_param(params, index)?;
    let text = text.strip_prefix("0x").unwrap_or(text);
    Vec::from_hex(text).map_err(|_| RpcError::params("Expected hex input"))
}

fn decode_transaction(hex: &str) -> Result<Transaction> {
    let bytes = Vec::from_hex(hex).map_err(|_| RpcError::new(-22, "TX decode failed"))?;
    deserialize(&bytes).map_err(|error| RpcError::new(-22, format!("TX decode failed: {error}")))
}

fn transaction_param(params: &Value) -> Result<Transaction> {
    decode_transaction(str_param(params, 0)?)
}

/// `sendrawtransaction`'s `maxfeerate` in BTC/kvB as satoshis per vbyte;
/// zero allows any fee.
fn max_fee_rate(value: Option<&Value>) -> Result<Option<u64>> {
    let btc_per_kvb = match value {
        None => DEFAULT_MAX_FEE_RATE,
        Some(value) => value
            .as_f64()
            .filter(|rate| rate.is_finite() && *rate >= 0.0)
            .ok_or_else(|| RpcError::params("Invalid maxfeerate"))?,
    };
    if btc_per_kvb == 0.0 {
        return Ok(None);
    }
    let per_kvb =
        Amount::from_btc(btc_per_kvb).map_err(|_| RpcError::params("Invalid maxfeerate"))?;
    Ok(Some((per_kvb.to_sat() / 1_000).max(1)))
}

/// Bitcoin Core takes `verbose` as a boolean or a number.
fn verbosity(value: Option<&Value>, default: u64) -> u64 {
    match value {
        Some(Value::Bool(verbose)) => u64::from(*verbose),
        Some(value) => value.as_u64().unwrap_or(default),
        None => default,
    }
}

fn block_param<'a>(runtime: &'a Runtime, params: &Value) -> Result<(u32, &'a Block)> {
    let hash = BlockHash::from_str(str_param(params, 0)?)
        .map_err(|_| RpcError::params("blockhash must be of length 64"))?;
    let height = runtime
        .chain()
        .height_of(&hash)
        .ok_or_else(|| RpcError::new(-5, "Block not found"))?;
    Ok((
        height,
        runtime
            .chain()
            .block(height)
            .expect("indexed heights exist"),
    ))
}

fn difficulty() -> f64 {
    // The regtest proof-of-work limit, which every Labcoat block uses.
    4.656542373906925e-10
}

fn median_time(runtime: &Runtime) -> u32 {
    let height = runtime.height();
    let mut times: Vec<u32> = (height.saturating_sub(10)..=height)
        .filter_map(|at| runtime.chain().block(at).map(|block| block.header.time))
        .collect();
    times.sort_unstable();
    times[times.len() / 2]
}

fn block_json(runtime: &Runtime, height: u32, block: &Block, transactions: bool) -> Value {
    let header = &block.header;
    let tx: Vec<Value> = if transactions {
        block.txdata.iter().map(transaction_json).collect()
    } else {
        block
            .txdata
            .iter()
            .map(|tx| tx.compute_txid().to_string().into())
            .collect()
    };
    let mut value = json!({
        "hash": block.block_hash().to_string(),
        "confirmations": runtime.height() - height + 1,
        "height": height,
        "version": header.version.to_consensus(),
        "versionHex": format!("{:08x}", header.version.to_consensus()),
        "merkleroot": header.merkle_root.to_string(),
        "time": header.time,
        "mediantime": header.time,
        "nonce": header.nonce,
        "bits": format!("{:08x}", header.bits.to_consensus()),
        "difficulty": difficulty(),
        "nTx": block.txdata.len(),
        "strippedsize": (block.weight().to_wu() - block.total_size() as u64) / 3,
        "size": block.total_size(),
        "weight": block.weight().to_wu(),
        "tx": tx,
    });
    if height > 0 {
        value["previousblockhash"] = header.prev_blockhash.to_string().into();
    }
    if let Some(next) = runtime.chain().block_hash(height + 1) {
        value["nextblockhash"] = next.to_string().into();
    }
    value
}

fn transaction_json(tx: &Transaction) -> Value {
    let coinbase = tx.is_coinbase();
    let vin: Vec<Value> = tx
        .input
        .iter()
        .map(|input| {
            let mut value = if coinbase {
                json!({ "coinbase": input.script_sig.to_hex_string() })
            } else {
                json!({
                    "txid": input.previous_output.txid.to_string(),
                    "vout": input.previous_output.vout,
                    "scriptSig": {
                        "asm": input.script_sig.to_asm_string(),
                        "hex": input.script_sig.to_hex_string(),
                    },
                })
            };
            if !input.witness.is_empty() {
                value["txinwitness"] = Value::from(
                    input
                        .witness
                        .iter()
                        .map(|item| item.to_lower_hex_string())
                        .collect::<Vec<_>>(),
                );
            }
            value["sequence"] = input.sequence.to_consensus_u32().into();
            value
        })
        .collect();
    let vout: Vec<Value> = tx
        .output
        .iter()
        .enumerate()
        .map(|(n, output)| {
            json!({
                "value": output.value.to_btc(),
                "n": n,
                "scriptPubKey": script_json(&output.script_pubkey),
            })
        })
        .collect();
    json!({
        "txid": tx.compute_txid().to_string(),
        "hash": tx.compute_wtxid().to_string(),
        "version": tx.version.0,
        "size": tx.total_size(),
        "vsize": tx.vsize(),
        "weight": tx.weight().to_wu(),
        "locktime": tx.lock_time.to_consensus_u32(),
        "vin": vin,
        "vout": vout,
        "hex": serialize(tx).to_lower_hex_string(),
    })
}

fn script_json(script: &Script) -> Value {
    let kind = if script.is_p2tr() {
        "witness_v1_taproot"
    } else if script.is_p2wpkh() {
        "witness_v0_keyhash"
    } else if script.is_p2wsh() {
        "witness_v0_scripthash"
    } else if script.is_p2pkh() {
        "pubkeyhash"
    } else if script.is_p2sh() {
        "scripthash"
    } else if script.is_op_return() {
        "nulldata"
    } else if script.is_p2pk() {
        "pubkey"
    } else if script.is_witness_program() {
        "witness_unknown"
    } else {
        "nonstandard"
    };
    let mut value = json!({
        "asm": script.to_asm_string(),
        "hex": script.to_hex_string(),
        "type": kind,
    });
    if let Ok(address) = Address::from_script(script, Network::Regtest) {
        value["address"] = address.to_string().into();
    }
    value
}
