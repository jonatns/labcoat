//! Esplora's address and transaction queries, answered from the ledger.
//!
//! These replace the Esplorashrew indexer module. Each view takes the text a
//! client sends to a Qubitcoin node's `secondaryview esplora` method and
//! returns the same bytes Esplorashrew returned: JSON for most views, plain
//! text for `txhex`, `tipheight`, `tiphash`, and `blockheight`. Transaction
//! objects follow Esplora's REST API, including each input's `prevout`.

use bitcoin::consensus::serialize;
use bitcoin::hex::{DisplayHex, FromHex};
use bitcoin::{Address, BlockHash, Network, OutPoint, Script, Transaction, TxOut, Txid};
use serde_json::{json, Value};
use std::str::FromStr;

use crate::ledger::Location;
use crate::Runtime;

/// Answer the Esplora view `name` for the request text `input`.
pub fn view(runtime: &Runtime, name: &str, input: &[u8]) -> Result<Vec<u8>, String> {
    let input = std::str::from_utf8(input)
        .map_err(|_| "the request is not text".to_owned())?
        .trim();
    let json = |value: Value| Ok(value.to_string().into_bytes());
    let text = |value: String| Ok(value.into_bytes());

    match name {
        "utxosbyscripthash" => json(utxos(runtime, input)?),
        "tx" => match find(runtime, input) {
            Some((tx, location)) => json(transaction(runtime, tx, location)),
            None => json(json!({ "error": "tx not found" })),
        },
        "txhex" => match find(runtime, input) {
            Some((tx, _)) => text(serialize(tx).to_lower_hex_string()),
            None => text("tx not found".into()),
        },
        "txraw" => match find(runtime, input) {
            Some((tx, _)) => Ok(serialize(tx)),
            None => text("tx not found".into()),
        },
        "txstatus" => json(match find(runtime, input) {
            Some((_, location)) => status(runtime, location),
            None => json!({ "confirmed": false }),
        }),
        "txoutspend" => json(outspend(runtime, input)?),
        "tipheight" => text(runtime.height().to_string()),
        "tiphash" => text(runtime.chain().tip().block_hash().to_string()),
        "blockheight" => match input
            .parse()
            .ok()
            .and_then(|height| runtime.chain().block_hash(height))
        {
            Some(hash) => text(hash.to_string()),
            None => text("block not found".into()),
        },
        "block" | "blockstatus" | "blocktxids" | "blockheader" => {
            let Some(height) = BlockHash::from_str(input)
                .ok()
                .and_then(|hash| runtime.chain().height_of(&hash))
            else {
                return json(json!({ "error": "block not found" }));
            };
            let block = runtime
                .chain()
                .block(height)
                .expect("indexed heights exist");
            match name {
                "block" => json(json!({
                    "id": block.block_hash().to_string(),
                    "height": height,
                    "version": block.header.version.to_consensus(),
                    "timestamp": block.header.time,
                    "tx_count": block.txdata.len(),
                    "size": block.total_size(),
                    "weight": block.weight().to_wu(),
                    "merkle_root": block.header.merkle_root.to_string(),
                    "previousblockhash": (height > 0).then(|| block.header.prev_blockhash.to_string()),
                    "nonce": block.header.nonce,
                    "bits": block.header.bits.to_consensus(),
                    "difficulty": 1,
                })),
                "blockstatus" => json(json!({
                    "in_best_chain": true,
                    "height": height,
                    "next_best": runtime.chain().block_hash(height + 1).map(|hash| hash.to_string()),
                })),
                "blocktxids" => json(Value::from(
                    block
                        .txdata
                        .iter()
                        .map(|tx| tx.compute_txid().to_string())
                        .collect::<Vec<_>>(),
                )),
                _ => text(serialize(&block.header).to_lower_hex_string()),
            }
        }
        _ => Err(format!("unknown esplora view '{name}'")),
    }
}

/// The confirmed unspent outputs paying a script, given the hex of its
/// SHA-256 in reversed byte order, as Electrum and Esplora write it.
fn utxos(runtime: &Runtime, input: &str) -> Result<Value, String> {
    let mut hash = <[u8; 32]>::from_hex(input).map_err(|_| "invalid script hash".to_owned())?;
    hash.reverse();
    let ledger = runtime.ledger();
    let mut coins: Vec<_> = ledger
        .coins_for(&hash)
        .into_iter()
        .map(|(outpoint, coin)| {
            let position = ledger.location(&outpoint.txid).map_or(0, |at| at.position);
            (
                (coin.height, position, outpoint.vout),
                outpoint,
                coin.output.value,
            )
        })
        .collect();
    coins.sort_by_key(|(order, ..)| *order);
    Ok(Value::from(
        coins
            .into_iter()
            .map(|((height, position, _), outpoint, value)| {
                json!({
                    "txid": outpoint.txid.to_string(),
                    "vout": outpoint.vout,
                    "value": value.to_sat(),
                    "status": status(runtime, Some(Location { height, position })),
                })
            })
            .collect::<Vec<_>>(),
    ))
}

fn outspend(runtime: &Runtime, input: &str) -> Result<Value, String> {
    let (txid, vout) = input
        .split_once(['/', ':'])
        .ok_or_else(|| "invalid input".to_owned())?;
    let outpoint = OutPoint::new(
        Txid::from_str(txid).map_err(|_| "invalid input".to_owned())?,
        vout.parse().map_err(|_| "invalid input".to_owned())?,
    );
    if let Some(spender) = runtime.ledger().spender(&outpoint) {
        return Ok(json!({
            "spent": true,
            "txid": spender.txid.to_string(),
            "vin": spender.input,
            "status": status(runtime, runtime.ledger().location(&spender.txid)),
        }));
    }
    if let Some(txid) = runtime.mempool().spender(&outpoint) {
        let entry = runtime
            .mempool()
            .get(&txid)
            .expect("the mempool indexes its spenders");
        let vin = entry
            .tx
            .input
            .iter()
            .position(|input| input.previous_output == outpoint)
            .unwrap_or(0);
        return Ok(json!({
            "spent": true,
            "txid": txid.to_string(),
            "vin": vin,
            "status": { "confirmed": false },
        }));
    }
    Ok(json!({ "spent": false }))
}

/// A confirmed or mempool transaction and where it is.
pub fn find<'a>(runtime: &'a Runtime, txid: &str) -> Option<(&'a Transaction, Option<Location>)> {
    let txid = Txid::from_str(txid).ok()?;
    if let Some(location) = runtime.ledger().location(&txid) {
        let block = runtime.chain().block(location.height)?;
        return Some((&block.txdata[location.position as usize], Some(location)));
    }
    runtime.mempool().get(&txid).map(|entry| (&entry.tx, None))
}

/// The output an outpoint names, whether spent or not.
pub fn previous_output(runtime: &Runtime, outpoint: &OutPoint) -> Option<TxOut> {
    let (tx, _) = find(runtime, &outpoint.txid.to_string())?;
    tx.output.get(outpoint.vout as usize).cloned()
}

pub fn status(runtime: &Runtime, location: Option<Location>) -> Value {
    match location.and_then(|at| Some((at.height, runtime.chain().block(at.height)?))) {
        Some((height, block)) => json!({
            "confirmed": true,
            "block_height": height,
            "block_hash": block.block_hash().to_string(),
            "block_time": block.header.time,
        }),
        None => json!({ "confirmed": false }),
    }
}

fn transaction(runtime: &Runtime, tx: &Transaction, location: Option<Location>) -> Value {
    let coinbase = tx.is_coinbase();
    let mut input_value = 0u64;
    let vin: Vec<Value> = tx
        .input
        .iter()
        .map(|input| {
            let prevout = (!coinbase)
                .then(|| previous_output(runtime, &input.previous_output))
                .flatten();
            if let Some(output) = &prevout {
                input_value += output.value.to_sat();
            }
            let mut value = json!({
                "txid": input.previous_output.txid.to_string(),
                "vout": input.previous_output.vout,
                "prevout": prevout.as_ref().map(output),
                "scriptsig": input.script_sig.to_hex_string(),
                "is_coinbase": coinbase,
                "sequence": input.sequence.to_consensus_u32(),
            });
            if !input.witness.is_empty() {
                value["witness"] = Value::from(
                    input
                        .witness
                        .iter()
                        .map(|item| item.to_lower_hex_string())
                        .collect::<Vec<_>>(),
                );
            }
            value
        })
        .collect();
    let output_value: u64 = tx.output.iter().map(|output| output.value.to_sat()).sum();
    json!({
        "txid": tx.compute_txid().to_string(),
        "version": tx.version.0,
        "locktime": tx.lock_time.to_consensus_u32(),
        "vin": vin,
        "vout": tx.output.iter().map(output).collect::<Vec<_>>(),
        "size": tx.total_size(),
        "weight": tx.weight().to_wu(),
        "fee": if coinbase { 0 } else { input_value.saturating_sub(output_value) },
        "status": status(runtime, location),
    })
}

fn output(output: &TxOut) -> Value {
    let mut value = json!({
        "scriptpubkey": output.script_pubkey.to_hex_string(),
        "scriptpubkey_type": script_type(&output.script_pubkey),
        "value": output.value.to_sat(),
    });
    if let Ok(address) = Address::from_script(&output.script_pubkey, Network::Regtest) {
        value["scriptpubkey_address"] = Value::from(address.to_string());
    }
    value
}

fn script_type(script: &Script) -> &'static str {
    if script.is_p2tr() {
        "v1_p2tr"
    } else if script.is_p2wpkh() {
        "v0_p2wpkh"
    } else if script.is_p2wsh() {
        "v0_p2wsh"
    } else if script.is_p2pkh() {
        "p2pkh"
    } else if script.is_p2sh() {
        "p2sh"
    } else if script.is_op_return() {
        "op_return"
    } else if script.is_p2pk() {
        "p2pk"
    } else {
        "unknown"
    }
}
