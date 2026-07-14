//! `labcoat mcp serve` — a Model Context Protocol server over stdio.
//!
//! Exposes devnet control (isomer-core) and contract ops (labcoat-core)
//! as MCP tools. Same typed functions as the CLI subcommands — no new
//! logic, just a JSON-RPC 2.0 shell (newline-delimited, per the MCP
//! stdio transport).

use crate::contract::{self, Ctx};
use isomer_core::Devnet;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2024-11-05";

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
        }
    })
}

fn tools() -> Vec<Value> {
    let arg_array = json!({
        "type": "array", "items": {"type": "string"},
        "description": "cellpack args: decimal u128, 0x-hex, or short strings (≤16 bytes)"
    });
    vec![
        tool("devnet_up", "Boot the full Alkanes devnet stack (downloads binaries when missing). Returns service status and the endpoint manifest.",
            json!({"noDownload": {"type": "boolean", "description": "skip the binary check/download"}}), &[]),
        tool("devnet_down", "Stop all devnet services.", json!({}), &[]),
        tool("devnet_status", "Devnet service health, block height, and mempool size.", json!({}), &[]),
        tool("devnet_mine", "Mine blocks on the devnet.",
            json!({"count": {"type": "integer", "minimum": 1, "maximum": 1000}, "address": {"type": "string"}}), &["count"]),
        tool("devnet_fund", "Send BTC from the devnet faucet wallet to an address.",
            json!({"address": {"type": "string"}, "amount": {"type": "number", "description": "BTC, defaults to 1"}}), &["address"]),
        tool("devnet_reset", "Stop services and wipe all devnet chain data.", json!({}), &[]),
        tool("devnet_logs", "Recent devnet service logs.",
            json!({"service": {"type": "string", "enum": ["bitcoind","metashrew","ord","esplora","espo","jsonrpc"]}, "limit": {"type": "integer"}}), &[]),
        tool("wallet_init", "Create or load the project wallet keystore. Optional mnemonic (else generated).",
            json!({"mnemonic": {"type": "string"}}), &[]),
        tool("wallet_addresses", "Wallet receive addresses per script type.",
            json!({"count": {"type": "integer", "minimum": 1}}), &[]),
        tool("wallet_utxos", "Spendable wallet UTXOs.", json!({}), &[]),
        tool("compile", "Compile a contract .rs file (or directory) to build/<name>.{wasm,wasm.gz,abi.json}.",
            json!({"path": {"type": "string"}, "name": {"type": "string"}, "outDir": {"type": "string"}}), &["path"]),
        tool("deploy", "Deploy a compiled contract (raw .wasm) via commit/reveal. Records it in labcoat.lock.",
            json!({"wasm": {"type": "string", "description": "path to the raw .wasm"}, "name": {"type": "string"}, "args": arg_array.clone()}), &["wasm"]),
        tool("call", "Execute a state-changing contract call and wait for its trace.",
            json!({"contract": {"type": "string", "description": "labcoat.lock name or block:tx id"}, "opcode": {"type": "string"}, "args": arg_array.clone()}), &["contract", "opcode"]),
        tool("simulate", "Read-only simulation of a contract call (no transaction).",
            json!({"contract": {"type": "string"}, "opcode": {"type": "string"}, "args": arg_array}), &["contract", "opcode"]),
        tool("trace", "Decoded protostone traces for a transaction.",
            json!({"txid": {"type": "string"}, "wait": {"type": "boolean"}}), &["txid"]),
    ]
}

fn str_args(v: Option<&Value>) -> Vec<String> {
    v.and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn dispatch(ctx: &Ctx, name: &str, args: &Value) -> Result<Value, (String, String)> {
    let fail = |e: contract::EnvelopeError| (format!("[{}] {}", e.code, e.message), e.hint.to_string());

    match name {
        "devnet_up" => {
            let mut devnet = Devnet::new();
            if !args.get("noDownload").and_then(|v| v.as_bool()).unwrap_or(false) {
                devnet
                    .ensure_binaries(|_, _| {})
                    .await
                    .map_err(|e| (e, "check network access to the binary hosts".into()))?;
            }
            devnet
                .start()
                .map_err(|e| (e, "see devnet_logs for the failing service".into()))?;
            let status = devnet.status().await;
            let endpoints = devnet.endpoints();
            std::mem::forget(devnet); // services must outlive this process
            Ok(json!({ "status": status, "endpoints": endpoints }))
        }
        "devnet_down" => {
            let mut devnet = Devnet::new();
            devnet
                .stop()
                .map_err(|e| (e, "check devnet_status".into()))?;
            Ok(json!({ "stopped": true }))
        }
        "devnet_status" => {
            let mut devnet = Devnet::new();
            Ok(serde_json::to_value(devnet.status().await).unwrap())
        }
        "devnet_mine" => {
            let devnet = Devnet::new();
            let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
            let address = args.get("address").and_then(|v| v.as_str()).map(String::from);
            let height = devnet
                .mine(count, address)
                .await
                .map_err(|e| (e, "is the devnet up? try devnet_status".into()))?;
            Ok(json!({ "mined": count, "height": height }))
        }
        "devnet_fund" => {
            let devnet = Devnet::new();
            let address = args.get("address").and_then(|v| v.as_str()).unwrap_or_default();
            let amount = args.get("amount").and_then(|v| v.as_f64()).unwrap_or(1.0);
            let txid = devnet
                .fund(address, amount)
                .await
                .map_err(|e| (e, "is the devnet up? try devnet_status".into()))?;
            Ok(json!({ "txid": txid }))
        }
        "devnet_reset" => {
            let mut devnet = Devnet::new();
            devnet
                .reset()
                .map_err(|e| (e, "check devnet_logs".into()))?;
            Ok(json!({ "reset": true }))
        }
        "devnet_logs" => {
            let devnet = Devnet::new();
            let service = args.get("service").and_then(|v| v.as_str()).map(String::from);
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
            Ok(serde_json::to_value(devnet.logs(service, limit)).unwrap())
        }
        "wallet_init" => {
            let mnemonic = args.get("mnemonic").and_then(|v| v.as_str()).map(String::from);
            let passphrase = ctx.passphrase();
            let res = async {
                ctx.config.require_passphrase_policy(&passphrase)?;
                let mut provider =
                    labcoat_core::system::connect(&ctx.config, passphrase.clone(), false).await?;
                labcoat_core::wallet::init(&mut provider, &ctx.config, mnemonic, passphrase).await
            }
            .await;
            res.map(|v| serde_json::to_value(v).unwrap())
                .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))
        }
        "wallet_addresses" => {
            let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
            let res = async {
                let provider =
                    labcoat_core::system::connect(&ctx.config, ctx.passphrase(), true).await?;
                labcoat_core::wallet::addresses(&provider, count).await
            }
            .await;
            res.map(|v| serde_json::to_value(v).unwrap())
                .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))
        }
        "wallet_utxos" => {
            let res = async {
                let provider =
                    labcoat_core::system::connect(&ctx.config, ctx.passphrase(), true).await?;
                labcoat_core::wallet::utxos(&provider).await
            }
            .await;
            res.map(|v| serde_json::to_value(v).unwrap())
                .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))
        }
        "compile" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or_default();
            let name = args.get("name").and_then(|v| v.as_str()).map(String::from);
            let out_dir = args.get("outDir").and_then(|v| v.as_str()).unwrap_or("build");
            let (_, res) = contract::compile(path, name, out_dir);
            res.map_err(fail)
        }
        "deploy" => {
            let wasm = args.get("wasm").and_then(|v| v.as_str()).unwrap_or_default();
            let name = args.get("name").and_then(|v| v.as_str()).map(String::from);
            let (_, res) = contract::deploy(ctx, wasm, name, &str_args(args.get("args"))).await;
            res.map_err(fail)
        }
        "call" | "simulate" => {
            let contract_ref = args.get("contract").and_then(|v| v.as_str()).unwrap_or_default();
            let opcode: u128 = args
                .get("opcode")
                .map(|v| match v {
                    Value::String(s) => s.parse().unwrap_or(0),
                    other => other.as_u64().unwrap_or(0) as u128,
                })
                .unwrap_or(0);
            let call_args = str_args(args.get("args"));
            let (_, res) = if name == "call" {
                contract::call(ctx, contract_ref, opcode, &call_args).await
            } else {
                contract::simulate(ctx, contract_ref, opcode, &call_args).await
            };
            res.map_err(fail)
        }
        "trace" => {
            let txid = args.get("txid").and_then(|v| v.as_str()).unwrap_or_default();
            let wait = args.get("wait").and_then(|v| v.as_bool()).unwrap_or(false);
            let (_, res) = contract::trace(ctx, txid, wait).await;
            res.map_err(fail)
        }
        other => Err((format!("unknown tool: {}", other), "call tools/list".into())),
    }
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Serve MCP over stdio until stdin closes.
pub async fn serve(ctx: Ctx) -> i32 {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");

        // Notifications (no id) need no response.
        if msg.get("id").is_none() {
            continue;
        }

        let response = match method {
            "initialize" => rpc_result(
                id,
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "labcoat", "version": env!("CARGO_PKG_VERSION") },
                }),
            ),
            "ping" => rpc_result(id, json!({})),
            "tools/list" => rpc_result(id, json!({ "tools": tools() })),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or_default();
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let empty = json!({});
                let args = params.get("arguments").unwrap_or(&empty);
                match dispatch(&ctx, name, args).await {
                    Ok(result) => rpc_result(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap() }],
                            "isError": false,
                        }),
                    ),
                    Err((message, hint)) => rpc_result(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": format!("{}\nhint: {}", message, hint) }],
                            "isError": true,
                        }),
                    ),
                }
            }
            other => rpc_error(id, -32601, &format!("method not found: {}", other)),
        };

        let mut bytes = serde_json::to_vec(&response).unwrap();
        bytes.push(b'\n');
        if stdout.write_all(&bytes).await.is_err() {
            break;
        }
        let _ = stdout.flush().await;
    }
    0
}
