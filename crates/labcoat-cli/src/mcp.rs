//! `labcoat mcp serve` — a Model Context Protocol server over stdio.
//!
//! Exposes Labcoat Network control (isomer-core) and contract ops (labcoat-core)
//! as MCP tools. Same typed functions as the CLI subcommands — no new
//! logic, just a JSON-RPC 2.0 shell (newline-delimited, per the MCP
//! stdio transport).

use crate::contract::{self, Ctx};
use isomer_core::LabcoatNetwork;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(crate) const PROTOCOL_VERSION: &str = "2024-11-05";

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

pub(crate) fn tools() -> Vec<Value> {
    let arg_array = json!({
        "type": "array", "items": {"type": "string"},
        "description": "cellpack args: decimal u128, 0x-hex, or short strings (≤16 bytes)"
    });
    vec![
        tool("network_up", "Boot Labcoat Network using the exact runtime bundle for this CLI release. Returns service status and the endpoint manifest.",
            json!({"noDownload": {"type": "boolean", "description": "skip the binary check/download"}}), &[]),
        tool("network_down", "Stop all Labcoat Network services.", json!({}), &[]),
        tool("network_status", "Labcoat Network service health, block height, and mempool size.", json!({}), &[]),
        tool("network_mine", "Mine blocks on Labcoat Network.",
            json!({"count": {"type": "integer", "minimum": 1, "maximum": 1000}, "address": {"type": "string"}}), &["count"]),
        tool("network_fund", "Send BTC from the Labcoat Network faucet wallet to an address.",
            json!({"address": {"type": "string"}, "amount": {"type": "number", "description": "BTC, defaults to 1"}}), &["address"]),
        tool("network_reset", "Stop services and wipe all Labcoat Network chain data.", json!({}), &[]),
        tool("network_logs", "Recent Labcoat Network service logs.",
            json!({"service": {"type": "string", "enum": ["qubitcoind"]}, "limit": {"type": "integer"}}), &[]),
        tool("wallet_init", "Create or load the project wallet keystore. Optional mnemonic (else generated). Generated mnemonics are redacted from the response unless showMnemonic is true.",
            json!({"mnemonic": {"type": "string"}, "showMnemonic": {"type": "boolean"}}), &[]),
        tool("wallet_addresses", "Wallet receive addresses per script type.",
            json!({"count": {"type": "integer", "minimum": 1}}), &[]),
        tool("wallet_utxos", "Spendable wallet UTXOs.", json!({}), &[]),
        tool("build", "Build Cargo contract packages and extract their Wasm-exported ABIs.",
            json!({"package": {"type": "string"}, "outDir": {"type": "string"}}), &[]),
        tool("test", "Build every contract for WASIp1 and run host integration tests; the first build may take several minutes.",
            json!({"package": {"type": "string"}}), &[]),
        tool("abi_fetch", "Fetch ABI metadata from the in-process Alkanes indexer.",
            json!({"contract": {"type": "string"}, "out": {"type": "string"}}), &["contract"]),
        tool("abi_verify", "Compare a deployed ABI with a locally built contract package.",
            json!({"contract": {"type": "string"}, "package": {"type": "string"}}), &["contract"]),
        tool("deploy", "Build and deploy an exact Cargo contract package, or deploy an explicit raw Wasm. Provide exactly one of package or wasm.",
            json!({"package": {"type": "string", "description": "exact Cargo contract package name"}, "wasm": {"type": "string", "description": "explicit path to raw .wasm; skips compilation"}, "name": {"type": "string", "description": "optional name for wasm deployments"}, "args": arg_array.clone(), "reserve": {"type": "string", "description": "reserved number N for a [3,N] deploy target (default: next free id via [1,0])"}, "inputs": {"type": "string", "description": "comma-separated extra inputs: alkanes block:tx:amount (0 = all) or bitcoin B:sats"}, "to": {"type": "string", "description": "recipient address for protostone outputs (default: wallet primary address)"}, "pointer": {"type": "string", "description": "protostone pointer target vN or pN (default v0)"}, "refund": {"type": "string", "description": "protostone refund target (default: pointer)"}, "edicts": {"type": "array", "items": {"type": "string"}, "description": "edicts block:tx:amount:target appended to the protostone"}}), &[]),
        tool("call", "Execute a state-changing contract call and wait for its trace.",
            json!({"contract": {"type": "string", "description": "labcoat.lock name or block:tx id"}, "opcode": {"type": "string", "description": "exact ABI method name or decimal opcode"}, "args": arg_array.clone(), "inputs": {"type": "string", "description": "comma-separated extra inputs: alkanes block:tx:amount (0 = all) or bitcoin B:sats"}, "to": {"type": "string", "description": "recipient address for protostone outputs (default: wallet primary address)"}, "pointer": {"type": "string", "description": "protostone pointer target vN or pN (default v0)"}, "refund": {"type": "string", "description": "protostone refund target (default: pointer)"}, "edicts": {"type": "array", "items": {"type": "string"}, "description": "edicts block:tx:amount:target appended to the protostone"}}), &["contract", "opcode"]),
        tool("exchange_plan", "Build an owner-partitioned atomic exchange plan and return its base64 PSBT.",
            json!({"offered": {"type": "string"}, "offeredAmount": {"type": "integer", "minimum": 1}, "payment": {"type": "string"}, "paymentAmount": {"type": "integer", "minimum": 1}, "sellerAddress": {"type": "string"}, "buyerAddress": {"type": "string"}}), &["offered", "offeredAmount", "payment", "paymentAmount", "sellerAddress", "buyerAddress"]),
        tool("exchange_settle", "Validate a buyer-signed PSBT, sign seller inputs, and optionally broadcast. broadcast must be true to transact.",
            json!({"plan": {"type": "object"}, "psbt": {"type": "string", "description": "base64 or hex buyer-signed PSBT"}, "sellerWalletFile": {"type": "string"}, "broadcast": {"type": "boolean"}}), &["plan", "psbt", "sellerWalletFile", "broadcast"]),
        tool("simulate", "Simulate a deployed contract against live indexed chain state (no transaction).",
            json!({"contract": {"type": "string"}, "opcode": {"type": "string", "description": "exact ABI method name or decimal opcode"}, "args": arg_array}), &["contract", "opcode"]),
        tool("trace", "Decoded protostone traces for a transaction.",
            json!({"txid": {"type": "string"}, "wait": {"type": "boolean"}}), &["txid"]),
        tool("balance", "Alkanes token balances held by an address.",
            json!({"address": {"type": "string"}}), &["address"]),
        tool("plan", "Reconcile the alkanes.hcl deployment manifest against labcoat.lock and chain state; shows pending actions without loading a signer.",
            json!({"manifest": {"type": "string", "description": "manifest path (default alkanes.hcl)"}}), &[]),
        tool("apply", "Execute the deployment manifest's pending actions. Requires broadcast: true to transact; otherwise returns the plan.",
            json!({"manifest": {"type": "string", "description": "manifest path (default alkanes.hcl)"}, "broadcast": {"type": "boolean", "description": "actually broadcast transactions"}}), &[]),
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

/// Optional transaction-shaping params shared by the deploy and call tools.
fn tx_flags(args: &Value) -> contract::TxFlags {
    let string = |key: &str| args.get(key).and_then(|v| v.as_str()).map(String::from);
    contract::TxFlags {
        inputs: string("inputs"),
        to: string("to"),
        pointer: string("pointer"),
        refund: string("refund"),
        edicts: str_args(args.get("edicts")),
    }
}

/// The deploy tool's reserve target: a number or decimal string (u128).
fn parse_reserve(args: &Value) -> Result<Option<u128>, (String, String)> {
    match args.get("reserve") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) if n.as_u64().is_some() => Ok(n.as_u64().map(u128::from)),
        Some(Value::String(s)) if s.parse::<u128>().is_ok() => Ok(s.parse().ok()),
        Some(_) => Err((
            "[CONFIG_INVALID] reserve must be a decimal u128".into(),
            "pass the reserved number N for a [3,N] deploy target".into(),
        )),
    }
}

fn call_selector(args: &Value) -> Result<String, (String, String)> {
    match args.get("opcode") {
        Some(Value::String(selector)) if !selector.is_empty() => Ok(selector.clone()),
        Some(Value::Number(selector)) => Ok(selector.to_string()),
        Some(_) => Err((
            "[CONFIG_INVALID] opcode must be an ABI method name or decimal opcode".into(),
            "pass a non-empty string such as `increment` or `1`".into(),
        )),
        None => Err((
            "[CONFIG_INVALID] opcode is required".into(),
            "pass an ABI method name or decimal opcode in `opcode`".into(),
        )),
    }
}

async fn dispatch(ctx: &Ctx, name: &str, args: &Value) -> Result<Value, (String, String)> {
    let fail =
        |e: contract::EnvelopeError| (format!("[{}] {}", e.code, e.message), e.hint.to_string());

    match name {
        "network_up" => {
            let mut network = LabcoatNetwork::new();
            if !args
                .get("noDownload")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                network
                    .ensure_binaries(|_, _| {})
                    .await
                    .map_err(|e| (e, "check network access to the binary hosts".into()))?;
            }
            network
                .start()
                .map_err(|e| (e, "see network_logs for the failing service".into()))?;
            let status = network.status().await;
            let endpoints = network.endpoints();
            std::mem::forget(network); // services must outlive this process
            Ok(json!({ "status": status, "endpoints": endpoints }))
        }
        "network_down" => {
            let mut network = LabcoatNetwork::new();
            network
                .stop()
                .map_err(|e| (e, "check network_status".into()))?;
            Ok(json!({
                "network": "labcoat",
                "bitcoin_network": "regtest",
                "stopped": true
            }))
        }
        "network_status" => {
            let mut network = LabcoatNetwork::new();
            Ok(serde_json::to_value(network.status().await).unwrap())
        }
        "network_mine" => {
            let network = LabcoatNetwork::new();
            let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
            let address = args
                .get("address")
                .and_then(|v| v.as_str())
                .map(String::from);
            let height = network
                .mine(count, address)
                .await
                .map_err(|e| (e, "is Labcoat Network up? try network_status".into()))?;
            Ok(json!({
                "network": "labcoat",
                "bitcoin_network": "regtest",
                "mined": count,
                "height": height
            }))
        }
        "network_fund" => {
            let network = LabcoatNetwork::new();
            let address = args
                .get("address")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let amount = args.get("amount").and_then(|v| v.as_f64()).unwrap_or(1.0);
            let txid = network
                .fund(address, amount)
                .await
                .map_err(|e| (e, "is Labcoat Network up? try network_status".into()))?;
            Ok(json!({
                "network": "labcoat",
                "bitcoin_network": "regtest",
                "txid": txid
            }))
        }
        "network_reset" => {
            let mut network = LabcoatNetwork::new();
            network
                .reset()
                .map_err(|e| (e, "check network_logs".into()))?;
            Ok(json!({
                "network": "labcoat",
                "bitcoin_network": "regtest",
                "reset": true
            }))
        }
        "network_logs" => {
            let network = LabcoatNetwork::new();
            let service = args
                .get("service")
                .and_then(|v| v.as_str())
                .map(String::from);
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
            Ok(serde_json::to_value(network.logs(service, limit)).unwrap())
        }
        "wallet_init" => {
            let mnemonic = args
                .get("mnemonic")
                .and_then(|v| v.as_str())
                .map(String::from);
            let show_mnemonic = args
                .get("showMnemonic")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let passphrase = ctx.passphrase();
            let res: Result<labcoat_core::wallet::WalletInitResult, labcoat_core::LabcoatError> =
                async {
                    ctx.config.require_passphrase_policy(&passphrase)?;
                    let mut provider =
                        labcoat_core::system::connect(&ctx.config, passphrase.clone(), false)
                            .await?;
                    let mut result = labcoat_core::wallet::init(
                        &mut provider,
                        &ctx.config,
                        mnemonic,
                        passphrase,
                    )
                    .await?;
                    // MCP transcripts are logs; never capture a generated
                    // mnemonic in one unless the caller asked for it.
                    if !show_mnemonic && result.mnemonic.is_some() {
                        result.mnemonic = None;
                        result.mnemonic_redacted = true;
                    }
                    Ok(result)
                }
                .await;
            res.map(|v| serde_json::to_value(v).unwrap())
                .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))
        }
        "wallet_addresses" => {
            let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
            let res = async {
                let provider = ctx.wallet_provider().await?;
                labcoat_core::wallet::addresses(&provider, count).await
            }
            .await;
            res.map(|v| serde_json::to_value(v).unwrap())
                .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))
        }
        "wallet_utxos" => {
            let res = async {
                let provider = ctx.wallet_provider().await?;
                labcoat_core::wallet::utxos(&provider).await
            }
            .await;
            res.map(|v| serde_json::to_value(v).unwrap())
                .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))
        }
        "build" => {
            let package = args.get("package").and_then(|v| v.as_str());
            let out_dir = args
                .get("outDir")
                .and_then(|v| v.as_str())
                .unwrap_or("build");
            let (_, res) = contract::build(package, out_dir);
            res.map_err(fail)
        }
        "test" => {
            let package = args.get("package").and_then(|v| v.as_str());
            crate::test_command::run(package).map_err(fail)
        }
        "abi_fetch" => {
            let contract_ref = args
                .get("contract")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let out = args.get("out").and_then(|v| v.as_str()).map(String::from);
            let (_, res) = contract::abi(
                ctx,
                contract::AbiCmd::Fetch {
                    contract: contract_ref,
                    out,
                },
            )
            .await;
            res.map_err(fail)
        }
        "abi_verify" => {
            let contract_ref = args
                .get("contract")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let package = args
                .get("package")
                .and_then(|v| v.as_str())
                .map(String::from);
            let (_, res) = contract::abi(
                ctx,
                contract::AbiCmd::Verify {
                    contract: contract_ref,
                    package,
                },
            )
            .await;
            res.map_err(fail)
        }
        "deploy" => {
            let package = args.get("package").and_then(|v| v.as_str());
            let wasm = args.get("wasm").and_then(|v| v.as_str());
            let name = args.get("name").and_then(|v| v.as_str()).map(String::from);
            let reserve = parse_reserve(args)?;
            let (_, res) = contract::deploy(
                ctx,
                package,
                wasm,
                name,
                &str_args(args.get("args")),
                reserve,
                &tx_flags(args),
            )
            .await;
            res.map_err(fail)
        }
        "call" | "simulate" => {
            let contract_ref = args
                .get("contract")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let selector = call_selector(args)?;
            let call_args = str_args(args.get("args"));
            let (_, res) = if name == "call" {
                contract::call(ctx, contract_ref, &selector, &call_args, &tx_flags(args)).await
            } else {
                contract::simulate(ctx, contract_ref, &selector, &call_args).await
            };
            res.map_err(fail)
        }
        "exchange_plan" => {
            let offered = args
                .get("offered")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let payment = args
                .get("payment")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let offered_amount = args
                .get("offeredAmount")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let payment_amount = args
                .get("paymentAmount")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let seller_address = args
                .get("sellerAddress")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let buyer_address = args
                .get("buyerAddress")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let res = async {
                let offered = contract::resolve(&ctx.config, offered)?;
                let payment = contract::resolve(&ctx.config, payment)?;
                let mut provider = ctx.wallet_provider().await?;
                labcoat_core::atomic_exchange::build_exchange_plan(
                    &mut provider,
                    &ctx.config,
                    labcoat_core::atomic_exchange::AtomicExchangeRequest {
                        offered: labcoat_core::atomic_exchange::AlkaneId {
                            block: u64::try_from(offered.0).map_err(|_| {
                                labcoat_core::LabcoatError::new(
                                    "CONFIG_INVALID",
                                    "offered block does not fit u64",
                                    "use a valid Alkane ID",
                                )
                            })?,
                            tx: u64::try_from(offered.1).map_err(|_| {
                                labcoat_core::LabcoatError::new(
                                    "CONFIG_INVALID",
                                    "offered tx does not fit u64",
                                    "use a valid Alkane ID",
                                )
                            })?,
                        },
                        offered_amount,
                        payment: labcoat_core::atomic_exchange::AlkaneId {
                            block: u64::try_from(payment.0).map_err(|_| {
                                labcoat_core::LabcoatError::new(
                                    "CONFIG_INVALID",
                                    "payment block does not fit u64",
                                    "use a valid Alkane ID",
                                )
                            })?,
                            tx: u64::try_from(payment.1).map_err(|_| {
                                labcoat_core::LabcoatError::new(
                                    "CONFIG_INVALID",
                                    "payment tx does not fit u64",
                                    "use a valid Alkane ID",
                                )
                            })?,
                        },
                        payment_amount,
                        seller_address: seller_address.to_string(),
                        buyer_address: buyer_address.to_string(),
                    },
                )
                .await
            }
            .await;
            res.map(|plan| serde_json::to_value(plan).unwrap())
                .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))
        }
        "exchange_settle" => {
            if args.get("broadcast").and_then(Value::as_bool) != Some(true) {
                return Err((
                    "[CONFIG_INVALID] exchange_settle requires broadcast: true".into(),
                    "use exchange_plan for read-only inspection".into(),
                ));
            }
            let plan: labcoat_core::atomic_exchange::ExchangePlanV1 = serde_json::from_value(
                args.get("plan").cloned().unwrap_or(Value::Null),
            )
            .map_err(|e| {
                (
                    format!("[EXCHANGE_PLAN_INVALID] {e}"),
                    "pass the complete ExchangePlanV1 object".into(),
                )
            })?;
            let psbt = labcoat_core::signer::decode_psbt(
                args.get("psbt").and_then(Value::as_str).unwrap_or_default(),
            )
            .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))?;
            let seller_wallet_file = args
                .get("sellerWalletFile")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let res = async {
                let mut config = ctx.config.clone();
                config.wallet_file = std::path::PathBuf::from(seller_wallet_file);
                let connected =
                    labcoat_core::system::connect_signing(&config, &ctx.signer_spec()?).await?;
                let mut provider = connected.provider;
                labcoat_core::atomic_exchange::settle_exchange(
                    &mut provider,
                    connected.signer.as_ref(),
                    &config,
                    &plan,
                    psbt,
                    true,
                )
                .await
            }
            .await;
            res.map(|outcome| serde_json::to_value(outcome).unwrap())
                .map_err(|e| (format!("[{}] {}", e.code, e.message), e.hint.to_string()))
        }
        "trace" => {
            let txid = args
                .get("txid")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let wait = args.get("wait").and_then(|v| v.as_bool()).unwrap_or(false);
            let (_, res) = contract::trace(ctx, txid, wait).await;
            res.map_err(fail)
        }
        "balance" => {
            let address = args
                .get("address")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let (_, res) = contract::balance(ctx, address).await;
            res.map_err(fail)
        }
        "plan" => {
            let manifest = args.get("manifest").and_then(|v| v.as_str());
            let (_, res) = contract::plan(ctx, manifest).await;
            res.map_err(fail)
        }
        "apply" => {
            let manifest = args.get("manifest").and_then(|v| v.as_str());
            let broadcast = args
                .get("broadcast")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let (_, res) = contract::apply(ctx, manifest, broadcast).await;
            res.map_err(fail)
        }
        other => Err((format!("unknown tool: {other}"), "call tools/list".into())),
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
            other => rpc_error(id, -32601, &format!("method not found: {other}")),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_and_abi_tool_schemas_match_the_cli() {
        let tools = tools();
        let named = |name: &str| {
            tools
                .iter()
                .find(|tool| tool["name"] == name)
                .unwrap_or_else(|| panic!("missing MCP tool {name}"))
        };

        assert_eq!(named("build")["inputSchema"]["required"], json!([]));
        assert!(named("build")["inputSchema"]["properties"]["package"].is_object());
        assert_eq!(named("test")["inputSchema"]["required"], json!([]));
        assert_eq!(
            named("abi_fetch")["inputSchema"]["required"],
            json!(["contract"])
        );
        assert_eq!(
            named("abi_verify")["inputSchema"]["required"],
            json!(["contract"])
        );
        assert!(named("abi_verify")["inputSchema"]["properties"]["package"].is_object());
        assert_eq!(named("deploy")["inputSchema"]["required"], json!([]));
        assert!(named("deploy")["inputSchema"]["properties"]["package"].is_object());
        assert!(named("deploy")["inputSchema"]["properties"]["wasm"].is_object());
        assert_eq!(
            named("call")["inputSchema"]["properties"]["opcode"]["description"],
            "exact ABI method name or decimal opcode"
        );
        assert_eq!(
            named("simulate")["inputSchema"]["properties"]["opcode"]["description"],
            "exact ABI method name or decimal opcode"
        );
    }

    #[test]
    fn labcoat_network_tools_use_only_the_new_identifiers() {
        let registered_tools = tools();
        let names: Vec<&str> = registered_tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        for name in [
            "network_up",
            "network_down",
            "network_status",
            "network_mine",
            "network_fund",
            "network_reset",
            "network_logs",
        ] {
            assert!(names.contains(&name), "missing MCP tool {name}");
        }
        assert!(!names
            .iter()
            .any(|name| name.starts_with(concat!("dev", "net_"))));
    }

    #[test]
    fn mcp_call_selector_never_defaults_invalid_values_to_zero() {
        assert_eq!(
            call_selector(&json!({"opcode": "increment"})).unwrap(),
            "increment"
        );
        assert_eq!(call_selector(&json!({"opcode": "1"})).unwrap(), "1");
        assert_eq!(call_selector(&json!({"opcode": 2})).unwrap(), "2");
        assert!(call_selector(&json!({"opcode": ""})).is_err());
        assert!(call_selector(&json!({"opcode": true})).is_err());
        assert!(call_selector(&json!({})).is_err());
    }
}
