use crate::contract::{CmdResult, EnvelopeError};
use crate::trace_view;
use anstream::{AutoStream, ColorChoice};
use anstyle::{AnsiColor, Effects, Style};
use clap::ValueEnum;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde_json::Value;
use std::io::{IsTerminal, Write};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, Default, ValueEnum, PartialEq, Eq)]
pub enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub verbose: bool,
    pub color: ColorMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Success,
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Block {
    Headline(Tone, String),
    Fields(Vec<(String, String)>),
    Table(Vec<String>, Vec<Vec<String>>),
    Tree(Vec<trace_view::TraceLine>),
    Note(Tone, String),
    Secret(String),
    Text(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Document {
    blocks: Vec<Block>,
}

impl Document {
    fn headline(&mut self, tone: Tone, text: impl Into<String>) {
        self.blocks.push(Block::Headline(tone, text.into()));
    }

    fn fields(&mut self, fields: Vec<(impl Into<String>, impl Into<String>)>) {
        let fields = fields
            .into_iter()
            .map(|(label, value)| (label.into(), value.into()))
            .filter(|(_, value)| !value.is_empty() && value != "null")
            .collect::<Vec<_>>();
        if !fields.is_empty() {
            self.blocks.push(Block::Fields(fields));
        }
    }

    fn note(&mut self, tone: Tone, text: impl Into<String>) {
        self.blocks.push(Block::Note(tone, text.into()));
    }
}

pub struct Progress {
    bar: ProgressBar,
}

impl Progress {
    pub fn new(message: &str, enabled: bool) -> Self {
        let target = if enabled && std::io::stderr().is_terminal() {
            ProgressDrawTarget::stderr()
        } else {
            ProgressDrawTarget::hidden()
        };
        let bar = ProgressBar::with_draw_target(None, target);
        bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .expect("static progress template")
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.set_message(message.to_owned());
        bar.enable_steady_tick(Duration::from_millis(90));
        Self { bar }
    }

    pub fn finish(self) {
        self.bar.finish_and_clear();
    }
}

pub fn finish_contract(json: bool, command: &str, result: CmdResult, options: Options) -> i32 {
    if json {
        let envelope = contract_envelope(command, &result);
        println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
        return 0;
    }
    match result {
        Ok(value) => {
            print_value(command, &value, options);
            0
        }
        Err(error) => {
            print_typed_error(&error, options.color);
            1
        }
    }
}

pub fn finish(json: bool, command: &str, result: Result<Value, String>, options: Options) -> i32 {
    if json {
        let envelope = labcoat_network_envelope(command, &result);
        println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
        return 0;
    }
    match result {
        Ok(value) => {
            print_value(command, &value, options);
            0
        }
        Err(error) => {
            print_untyped_error(&error, options.color);
            1
        }
    }
}

fn labcoat_network_envelope(command: &str, result: &Result<Value, String>) -> Value {
    match result {
        Ok(value) => serde_json::json!({
            "ok": true,
            "command": command,
            "schema": format!("labcoat/v1/{command}"),
            "result": value,
        }),
        Err(error) => serde_json::json!({
            "ok": false,
            "command": command,
            "schema": "labcoat/v1/error",
            "error": {
                "code": "LABCOAT_NETWORK_ERROR",
                "message": error,
                "hint": "run `labcoat status` to inspect Labcoat Network"
            },
        }),
    }
}

pub fn contract_envelope(command: &str, result: &CmdResult) -> Value {
    match result {
        Ok(value) => serde_json::json!({
            "ok": true,
            "command": command,
            "schema": format!("labcoat/v1/{command}"),
            "result": value,
        }),
        Err(error) => serde_json::json!({
            "ok": false,
            "command": command,
            "schema": "labcoat/v1/error",
            "error": {
                "code": error.code,
                "message": error.message,
                "hint": error.hint,
            },
        }),
    }
}

pub fn print_value(command: &str, value: &Value, options: Options) {
    let width = if std::io::stdout().is_terminal() {
        std::env::var("COLUMNS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|width| *width > 0)
            .unwrap_or(100)
    } else {
        100
    };
    let document = build_document(command, value, options.verbose);
    let rendered = render(&document, width, true);
    let choice = color_choice(options.color);
    let mut stream = AutoStream::new(std::io::stdout(), choice);
    let _ = stream.write_all(rendered.as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
fn render_plain(command: &str, value: &Value, verbose: bool, width: usize) -> String {
    render(&build_document(command, value, verbose), width, false)
}

/// Say which network and endpoint a state-changing command is about to
/// write to, so a write to the wrong one is visible before it lands.
pub fn print_target(network: &str, rpc_url: &str, color: ColorMode) {
    write_stderr(
        &format!(
            "{}network:{} {network} ({})\n",
            style(Tone::Info),
            reset(),
            display_endpoint(rpc_url)
        ),
        color,
    );
}

/// An endpoint without the credentials a URL can carry: userinfo and the
/// query string, where hosted RPC providers put API keys.
fn display_endpoint(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or(url);
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => rest.split_at(index),
        None => (rest, ""),
    };
    let host = authority.rsplit('@').next().unwrap_or(authority);
    format!("{scheme}://{host}{path}")
}

pub fn print_warning(message: &str, color: ColorMode) {
    write_stderr(
        &format!("{}warning:{} {message}\n", style(Tone::Warning), reset()),
        color,
    );
}

fn print_typed_error(error: &EnvelopeError, color: ColorMode) {
    let message = format!(
        "{}error[{}]:{} {}\n{}hint:{} {}\n",
        style(Tone::Error),
        error.code,
        reset(),
        error.message,
        style(Tone::Info),
        reset(),
        error.hint
    );
    write_stderr(&message, color);
}

fn print_untyped_error(error: &str, color: ColorMode) {
    let message = format!("{}error:{} {error}\n", style(Tone::Error), reset());
    write_stderr(&message, color);
}

fn write_stderr(message: &str, color: ColorMode) {
    let mut stream = AutoStream::new(std::io::stderr(), color_choice(color));
    let _ = stream.write_all(message.as_bytes());
    let _ = stream.flush();
}

/// Whether diagnostics written straight to stderr (the tracing subscriber)
/// may carry ANSI styling. Follows the same policy as the rest of human
/// output: `--color always|never` wins; `auto` honours `NO_COLOR`/`CLICOLOR*`
/// and colors only when stderr is a terminal.
pub fn stderr_ansi(mode: ColorMode) -> bool {
    resolve_ansi(mode, || AutoStream::choice(&std::io::stderr()))
}

fn resolve_ansi(mode: ColorMode, auto: impl FnOnce() -> ColorChoice) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => auto() != ColorChoice::Never,
    }
}

fn color_choice(mode: ColorMode) -> ColorChoice {
    match mode {
        ColorMode::Auto => ColorChoice::Auto,
        ColorMode::Always => ColorChoice::Always,
        ColorMode::Never => ColorChoice::Never,
    }
}

fn build_document(command: &str, value: &Value, verbose: bool) -> Document {
    match command {
        "call" => execution_document(command, value, verbose),
        // Deploy reports deploy.rs steps (a plan under --dry-run, what ran
        // otherwise) or, for --wasm, one execution.
        "deploy" if value.get("actions").is_none() => execution_document(command, value, verbose),
        "deploy" if value.get("pending").is_none() => deploy_report_document(value),
        "plan" | "deploy" => plan_document(value),
        "simulate" => simulate_document(value, verbose),
        "trace" => trace_document(value, verbose),
        "status" | "up" => status_document(command, value, verbose),
        "doctor" => doctor_document(value),
        "balance" => balance_document(value),
        "binaries" => binaries_document(value, verbose),
        "wallet-addresses" => wallet_addresses_document(value),
        "wallet-utxos" => wallet_utxos_document(value, verbose),
        "wallet-init" => wallet_init_document(value),
        "build" | "test" => build_document_report(command, value, verbose),
        "lock-show" => lock_document(value, verbose),
        "state-list" => state_list_document(value, verbose),
        "state-show" => state_show_document(value, verbose),
        "state-migrate" => state_migrate_document(value),
        "logs" => logs_document(value, verbose),
        "snapshot" if value.get("snapshots").is_some() => snapshots_document(value),
        "abi-fetch" | "abi-verify" => abi_document(command, value, verbose),
        "init" | "new" => scaffold_document(command, value, verbose),
        _ => action_or_generic_document(command, value, verbose),
    }
}

fn execution_document(command: &str, value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    if value.get("dryRun").and_then(Value::as_bool) == Some(true) {
        document.headline(Tone::Info, format!("{} plan", title(command)));
        document.fields(fields(
            value,
            &[
                ("Network", "network"),
                ("Contract", "name"),
                ("Package", "package"),
                ("Target", "target"),
                ("Method", "method"),
                ("Opcode", "opcode"),
                ("Arguments", "cellpackArgs"),
                ("Wasm", "wasm"),
                ("Size", "wasmBytes"),
                ("SHA-256", "wasmSha256"),
            ],
        ));
        document.fields(payment_fields(value));
        document.fields(fields(value, &[("Would broadcast", "wouldBroadcast")]));
        return document;
    }

    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("success");
    let reverted = status == "revert";
    // No trace arrived in time: on a public network the transaction is
    // usually still waiting for a block.
    let unsettled = status != "revert" && status != "success";
    document.headline(
        if reverted {
            Tone::Error
        } else if unsettled {
            Tone::Warning
        } else {
            Tone::Success
        },
        format!(
            "{} {}",
            title(command),
            if reverted {
                "reverted"
            } else if unsettled {
                "sent, result not known yet"
            } else {
                "succeeded"
            }
        ),
    );
    let mut important = fields(
        value,
        &[
            ("Contract", "name"),
            ("Target", "target"),
            ("Method", "method"),
            ("Opcode", "opcode"),
            ("Alkanes ID", "alkanesId"),
            ("Alkanes ID", "alkanes_id"),
            ("Transaction", "txid"),
            ("Commit transaction", "commitTxid"),
            ("Fee", "fee"),
            ("Commit fee", "commitFee"),
            ("Reason", "revertReason"),
        ],
    );
    if let Some(fee) = value.get("fee").and_then(Value::as_u64) {
        replace_field(&mut important, "Fee", format!("{} sats", format_u64(fee)));
    }
    if let Some(fee) = value.get("commitFee").and_then(Value::as_u64) {
        replace_field(
            &mut important,
            "Commit fee",
            format!("{} sats", format_u64(fee)),
        );
    }
    if let Some(result) = value
        .get("traces")
        .and_then(trace_view::decoded_return_from_traces)
    {
        important.push(("Return".into(), result));
    }
    important.extend(payment_fields(value));
    document.fields(important);
    if verbose {
        document.fields(fields(
            value,
            &[
                ("ABI source", "abiSource"),
                ("Local build", "localBuildStatus"),
                ("Target revision", "targetRevision"),
            ],
        ));
        if let Some(traces) = value.get("traces") {
            document
                .blocks
                .push(Block::Tree(trace_view::normalize(traces)));
        }
    } else if unsettled {
        document.note(
            Tone::Warning,
            format!(
                "The transaction was broadcast but has no execution trace yet. Run `labcoat trace {} --wait` once it confirms.",
                string_at(value, "txid")
            ),
        );
    } else if value.get("traces").is_some() {
        let txid = string_at(value, "txid");
        if !txid.is_empty() {
            document.note(
                Tone::Info,
                format!("Run `labcoat trace {txid}` or add `--verbose` for trace details."),
            );
        }
    }
    document
}

fn simulate_document(value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let reverted = status == "revert";
    document.headline(
        if reverted { Tone::Error } else { Tone::Success },
        if reverted {
            "Simulation reverted"
        } else {
            "Simulation succeeded"
        },
    );
    let result = value
        .pointer("/decoded/string")
        .or_else(|| value.pointer("/decoded/uint"))
        .map(value_string)
        .or_else(|| value.get("data").map(value_string))
        .unwrap_or_else(|| "(empty)".into());
    let mut output = fields(
        value,
        &[
            ("Target", "target"),
            ("Method", "method"),
            ("Opcode", "opcode"),
            ("Reason", "error"),
        ],
    );
    output.push(("Result".into(), result));
    if let Some(gas) = value.get("gasUsed").and_then(Value::as_u64) {
        output.push(("Gas".into(), format_u64(gas)));
    }
    document.fields(output);
    if verbose {
        document.fields(fields(
            value,
            &[
                ("Raw data", "data"),
                ("ABI source", "abiSource"),
                ("Local build", "localBuildStatus"),
                ("Target revision", "targetRevision"),
            ],
        ));
    }
    document
}

fn trace_document(value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    document.headline(Tone::Info, "Transaction trace");
    document.fields(fields(value, &[("Transaction", "txid")]));
    let lines = trace_view::normalize(value.get("traces").unwrap_or(&Value::Null));
    document.blocks.push(Block::Tree(lines.clone()));
    if verbose {
        for line in lines {
            document.blocks.push(Block::Text(format!(
                "{}{}\n{}",
                "  ".repeat(line.depth),
                line.summary,
                indent(&line.raw, line.depth + 1)
            )));
        }
    }
    document
}

fn status_document(command: &str, value: &Value, verbose: bool) -> Document {
    let status = value.get("status").unwrap_or(value);
    let mut document = Document::default();
    let ready = status
        .get("is_ready")
        .or_else(|| status.get("isReady"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    document.headline(
        if ready { Tone::Success } else { Tone::Warning },
        if command == "up" && ready {
            "Labcoat Network is ready"
        } else if ready {
            "Labcoat Network ready"
        } else {
            "Labcoat Network not ready"
        },
    );
    let bitcoin_mode = status
        .get("bitcoin_network")
        .or_else(|| status.get("bitcoinNetwork"))
        .map(value_string)
        .unwrap_or_else(|| "regtest".to_string());
    document.fields(vec![
        ("Network", "Labcoat Network".to_string()),
        ("Bitcoin mode", format!("{bitcoin_mode} (local/private)")),
        (
            "Instance",
            value_string(
                status
                    .get("instance_id")
                    .or_else(|| status.get("instanceId"))
                    .unwrap_or(&Value::Null),
            ),
        ),
        (
            "Block height",
            value_string(
                status
                    .get("block_height")
                    .or_else(|| status.get("blockHeight"))
                    .unwrap_or(&Value::Null),
            ),
        ),
        (
            "Mempool",
            value_string(
                status
                    .get("mempool_size")
                    .or_else(|| status.get("mempoolSize"))
                    .unwrap_or(&Value::Null),
            ),
        ),
    ]);
    if let Some(wallet) = value.get("wallet") {
        match wallet.get("error").and_then(Value::as_str) {
            Some(error) => document.note(
                Tone::Warning,
                format!("The built-in wallet is not ready: {error}. Run `labcoat up` again."),
            ),
            None => document.fields(vec![
                ("Wallet", string_at(wallet, "address")),
                (
                    "Balance",
                    format!(
                        "{:.8} BTC",
                        wallet
                            .get("balanceSats")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as f64
                            / 100_000_000.0
                    ),
                ),
            ]),
        }
    }
    if let Some(signer) = status.get("dev_signer") {
        let available = signer.get("available").and_then(Value::as_bool) == Some(true);
        document.fields(vec![(
            "Dev signer",
            if available {
                "signs PSBTs for the built-in wallet (labcoat_dev_sign_psbt)".to_string()
            } else {
                format!("not available: {}", string_at(signer, "reason"))
            },
        )]);
    }
    if let Some(services) = status.get("services").and_then(Value::as_array) {
        let rows = services
            .iter()
            .map(|service| {
                vec![
                    value_string(service.get("name").unwrap_or(&Value::Null)),
                    status_mark(
                        service
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                    ),
                    value_string(service.get("port").unwrap_or(&Value::Null)),
                    value_string(service.get("version").unwrap_or(&Value::Null)),
                ]
            })
            .collect();
        document.blocks.push(Block::Table(
            vec![
                "Service".into(),
                "Status".into(),
                "Port".into(),
                "Version".into(),
            ],
            rows,
        ));
    }
    if verbose {
        if let Some(endpoints) = value.get("endpoints") {
            document.blocks.push(Block::Text("Endpoints".into()));
            document.blocks.extend(generic_blocks(endpoints));
        }
    } else if let Some(endpoint) = value.pointer("/endpoints/rpc").and_then(Value::as_str) {
        document.fields(vec![("JSON-RPC", endpoint)]);
    }
    document
}

fn doctor_document(value: &Value) -> Document {
    let mut document = Document::default();
    let checks = value
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let count = |status: &str| {
        checks
            .iter()
            .filter(|check| check.get("status").and_then(Value::as_str) == Some(status))
            .count()
    };
    let (failed, warned) = (count("fail"), count("warn"));
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    match (failed, warned) {
        (0, 0) => document.headline(Tone::Success, "Environment looks good"),
        (0, _) => document.headline(
            Tone::Success,
            format!("Environment looks good, with {}", plural(warned, "note")),
        ),
        _ => document.headline(
            Tone::Error,
            format!("Environment has {}", plural(failed, "problem")),
        ),
    }
    // One line per check. A table would need a column for hints that most
    // rows leave empty, and stops fitting the terminal as soon as one path
    // is long. A hint goes under the check it belongs to.
    let width = checks
        .iter()
        .map(|check| string_at(check, "name").chars().count())
        .max()
        .unwrap_or(0);
    let home = std::env::var("HOME").ok().filter(|home| home.len() > 1);
    let mut lines = Vec::new();
    for check in &checks {
        let mark = match check.get("status").and_then(Value::as_str) {
            Some("ok") => "✓",
            Some("warn") => "!",
            Some("fail") => "✗",
            _ => "?",
        };
        let mut detail = string_at(check, "detail");
        if let Some(home) = &home {
            detail = detail.replace(home.as_str(), "~");
        }
        lines.push(format!(
            "{mark} {:<width$}  {detail}",
            string_at(check, "name")
        ));
        let hint = string_at(check, "hint");
        if !hint.is_empty() {
            lines.push(format!("  {:<width$}  → {hint}", ""));
        }
    }
    document
        .blocks
        .push(Block::Text(format!("\n{}", lines.join("\n"))));
    document
}

fn binaries_document(value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    document.headline(Tone::Info, "Service binaries");
    let items = value.as_array().cloned().unwrap_or_default();
    let rows = items
        .into_iter()
        .map(|binary| {
            let status = binary.get("status").map(binary_status).unwrap_or_default();
            let mut row = vec![string_at(&binary, "service"), status];
            if verbose {
                row.push(string_at(&binary, "path"));
                row.push(string_at(&binary, "size_bytes"));
            }
            row
        })
        .collect();
    let mut headers = vec!["Service".into(), "Status".into()];
    if verbose {
        headers.extend(["Path".into(), "Bytes".into()]);
    }
    document.blocks.push(Block::Table(headers, rows));
    document
}

fn wallet_addresses_document(value: &Value) -> Document {
    let mut document = Document::default();
    document.headline(Tone::Info, "Wallet addresses");
    let rows = value
        .as_array()
        .into_iter()
        .flatten()
        .map(|address| {
            vec![
                string_at(address, "index"),
                string_at(address, "scriptType"),
                string_at(address, "derivationPath"),
                string_at(address, "address"),
            ]
        })
        .collect();
    document.blocks.push(Block::Table(
        vec![
            "#".into(),
            "Type".into(),
            "Derivation".into(),
            "Address".into(),
        ],
        rows,
    ));
    document
}

fn wallet_utxos_document(value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    let items = value.as_array().cloned().unwrap_or_default();
    document.headline(Tone::Info, format!("{} spendable UTXO(s)", items.len()));
    let rows = items
        .into_iter()
        .map(|utxo| {
            let mut flags = Vec::new();
            for (field, label) in [
                ("frozen", "frozen"),
                ("hasInscriptions", "inscriptions"),
                ("hasRunes", "runes"),
                ("hasAlkanes", "alkanes"),
                ("isCoinbase", "coinbase"),
            ] {
                if utxo.get(field).and_then(Value::as_bool) == Some(true) {
                    flags.push(label);
                }
            }
            let mut row = vec![
                format!("{}:{}", string_at(&utxo, "txid"), string_at(&utxo, "vout")),
                string_at(&utxo, "amount"),
                string_at(&utxo, "confirmations"),
                flags.join(", "),
            ];
            if verbose {
                row.push(string_at(&utxo, "address"));
            }
            row
        })
        .collect();
    let mut headers = vec![
        "Outpoint".into(),
        "Sats".into(),
        "Conf.".into(),
        "Flags".into(),
    ];
    if verbose {
        headers.push("Address".into());
    }
    document.blocks.push(Block::Table(headers, rows));
    document
}

fn wallet_init_document(value: &Value) -> Document {
    let mut document = Document::default();
    let created = value
        .get("created")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    document.headline(
        Tone::Success,
        if created {
            "Wallet created"
        } else {
            "Wallet loaded"
        },
    );
    let network = match value.get("network").and_then(Value::as_str) {
        Some("labcoat") => "Labcoat Network".to_string(),
        Some(network) => network.to_string(),
        None => "—".to_string(),
    };
    let mut wallet_fields = vec![("Network".to_string(), network)];
    if let Some(bitcoin_network) = value.get("bitcoinNetwork") {
        wallet_fields.push(("Bitcoin mode".to_string(), value_string(bitcoin_network)));
    }
    wallet_fields.extend(fields(
        value,
        &[("Address", "address"), ("Wallet file", "walletFile")],
    ));
    document.fields(wallet_fields);
    if let Some(mnemonic) = value.get("mnemonic").and_then(Value::as_str) {
        document.blocks.push(Block::Secret(mnemonic.into()));
    }
    document
}

fn build_document_report(command: &str, value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    document.headline(
        Tone::Success,
        if command == "test" {
            "Tests passed"
        } else {
            "Build succeeded"
        },
    );
    let contracts = value
        .get("contracts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !contracts.is_empty() {
        let rows = contracts
            .into_iter()
            .map(|contract| {
                let mut row = vec![string_at(&contract, "name")];
                if verbose {
                    row.extend([
                        string_at(&contract, "wasmPath"),
                        string_at(&contract, "abiPath"),
                        string_at(&contract, "wasmSha256"),
                    ]);
                }
                row
            })
            .collect();
        let mut headers = vec!["Contract".into()];
        if verbose {
            headers.extend(["Wasm".into(), "ABI".into(), "SHA-256".into()]);
        }
        document.blocks.push(Block::Table(headers, rows));
    }
    if value.get("e2e").and_then(Value::as_bool) == Some(true) {
        document.fields(fields(value, &[("E2E tests", "tests")]));
        if verbose {
            if let Some(network) = value.get("network") {
                document.fields(fields(
                    network,
                    &[
                        ("E2E network", "rpcUrl"),
                        ("Chain data", "dataHome"),
                        ("Deployment records", "recordsDir"),
                    ],
                ));
            }
        }
    }
    if verbose {
        document.fields(fields(value, &[("Artifact directory", "artifactDir")]));
        if let Some(output) = value.get("output").and_then(Value::as_str) {
            if !output.trim().is_empty() {
                document.blocks.push(Block::Text(output.trim_end().into()));
            }
        }
    }
    document
}

fn lock_document(value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    document.headline(Tone::Info, "Deployments");
    let mut rows = Vec::new();
    if let Some(networks) = value.get("networks").and_then(Value::as_object) {
        for (network, contracts) in networks {
            if let Some(contracts) = contracts.as_object() {
                for (name, deployment) in contracts {
                    let mut row = vec![
                        network.clone(),
                        name.clone(),
                        string_at(deployment, "alkanesId"),
                        string_at(deployment, "status"),
                        string_at(deployment, "txid"),
                    ];
                    if verbose {
                        row.push(string_at(deployment, "wasmSha256"));
                    }
                    rows.push(row);
                }
            }
        }
    }
    let mut headers = vec![
        "Network".into(),
        "Contract".into(),
        "Alkanes ID".into(),
        "Status".into(),
        "Transaction".into(),
    ];
    if verbose {
        headers.push("Wasm SHA-256".into());
    }
    document.blocks.push(Block::Table(headers, rows));
    document
}

fn state_list_document(value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    document.headline(
        Tone::Info,
        format!("Durable state ({})", string_at(value, "environment")),
    );
    document.fields(fields(
        value,
        &[("Serial", "serial"), ("Lineage", "lineage")],
    ));
    if let Some(chain) = value.get("chain") {
        document.fields(fields(
            chain,
            &[
                ("Network", "network"),
                ("Chain", "block1Hash"),
                ("Instance", "labcoatNetworkInstanceId"),
            ],
        ));
    }
    let mut rows = Vec::new();
    if let Some(resources) = value.get("resources").and_then(Value::as_array) {
        for resource in resources {
            let mut row = vec![
                string_at(resource, "address"),
                string_at(resource, "alkanesId"),
                string_at(resource, "status"),
                string_at(resource, "instances"),
            ];
            if verbose {
                row.push(string_at(resource, "activeInstance"));
            }
            rows.push(row);
        }
    }
    let mut headers = vec![
        "Resource".into(),
        "Alkanes ID".into(),
        "Status".into(),
        "Instances".into(),
    ];
    if verbose {
        headers.push("Active instance".into());
    }
    document.blocks.push(Block::Table(headers, rows));
    document
}

fn state_show_document(value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    document.headline(Tone::Info, string_at(value, "resource"));
    document.fields(fields(
        value,
        &[
            ("Environment", "environment"),
            ("Kind", "kind"),
            ("Active instance", "activeInstance"),
        ],
    ));
    let mut rows = Vec::new();
    if let Some(instances) = value.get("instances").and_then(Value::as_array) {
        for instance in instances {
            let mut row = vec![
                string_at(instance, "instanceId"),
                string_at(instance, "origin"),
                string_at(instance, "alkanesId"),
                string_at(instance, "status"),
                string_at(instance, "txid"),
            ];
            if verbose {
                row.push(string_at(instance, "wasmSha256"));
                row.push(string_at(instance, "chainId"));
            }
            rows.push(row);
        }
    }
    let mut headers = vec![
        "Instance".into(),
        "Origin".into(),
        "Alkanes ID".into(),
        "Status".into(),
        "Transaction".into(),
    ];
    if verbose {
        headers.push("Wasm SHA-256".into());
        headers.push("Chain".into());
    }
    document.blocks.push(Block::Table(headers, rows));
    document
}

fn state_migrate_document(value: &Value) -> Document {
    let mut document = Document::default();
    document.headline(Tone::Success, "Durable state created");
    document.fields(fields(
        value,
        &[
            ("Environment", "environment"),
            ("Network", "network"),
            ("State file", "statePath"),
            ("Backup", "backup"),
            ("Resources", "resources"),
            ("Instances", "instances"),
            ("Serial", "serial"),
            ("Lineage", "lineage"),
        ],
    ));
    document
}

/// The id a plan row shows: the contract's id once it exists, or the id it
/// will get when that is known in advance (a reserved number, a call's
/// target). A contract deployed without a reserved number is created
/// through `1:0`, which asks the protocol for the next free id; that is an
/// instruction, not an id, and printing it reads as a wrong prediction when
/// the deploy then reports `2:n`.
fn planned_id(action: &Value) -> String {
    if let Some(id) = action.get("id").and_then(Value::as_str) {
        return id.to_string();
    }
    match action.get("target").and_then(Value::as_str) {
        Some("1:0") => "assigned at deploy".into(),
        Some(target) => target.to_string(),
        None => "-".into(),
    }
}

fn plan_document(value: &Value) -> Document {
    let mut document = Document::default();
    let pending = value.get("pending").and_then(Value::as_u64).unwrap_or(0);
    let conflicts = value.get("conflicts").and_then(Value::as_u64).unwrap_or(0);
    let scope = match value.get("only").and_then(Value::as_str) {
        Some(only) => format!("{} step `{only}`", string_at(value, "script")),
        None => string_at(value, "script"),
    };
    document.headline(
        if conflicts > 0 {
            Tone::Warning
        } else {
            Tone::Info
        },
        if conflicts > 0 {
            format!("Plan for {scope} — {pending} pending, {conflicts} in conflict")
        } else {
            format!("Plan for {scope} — {pending} pending step(s)")
        },
    );
    document.fields(fields(
        value,
        &[
            ("Network", "network"),
            ("Height", "height"),
            ("Chain", "chainId"),
        ],
    ));
    let mut rows = Vec::new();
    let mut arguments = Vec::new();
    let mut payments = Vec::new();
    let mut details = Vec::new();
    for action in value
        .get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let status = string_at(action, "status");
        let change = action
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("none");
        let args = action
            .get("args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        // Arguments matter for what is about to run or needs a decision;
        // a completed step's are in the run file.
        if status != "complete" && !args.is_empty() {
            arguments.push(vec![string_at(action, "name"), args]);
        }
        if status != "complete" {
            for payment in action
                .get("payments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                // `address:sats`, as declared.
                let pays = match payment.rsplit_once(':') {
                    Some((address, sats)) => format!("{sats} sats to {address}"),
                    None => payment.to_string(),
                };
                payments.push(vec![string_at(action, "name"), pays]);
            }
        }
        rows.push(vec![
            string_at(action, "kind"),
            string_at(action, "name"),
            if status == "pending" && change != "none" {
                format!("{status} ({change})")
            } else {
                status
            },
            planned_id(action),
        ]);
        if let Some(detail) = action.get("detail").and_then(Value::as_str) {
            details.push((
                action.get("status").and_then(Value::as_str) == Some("conflict"),
                format!("{}: {detail}", string_at(action, "name")),
            ));
        }
    }
    if !rows.is_empty() {
        document.blocks.push(Block::Table(
            vec!["Kind".into(), "Name".into(), "Status".into(), "Id".into()],
            rows,
        ));
    }
    if !arguments.is_empty() {
        document.blocks.push(Block::Table(
            vec!["Step".into(), "Arguments".into()],
            arguments,
        ));
    }
    if !payments.is_empty() {
        document
            .blocks
            .push(Block::Table(vec!["Step".into(), "Pays".into()], payments));
    }
    for (conflict, detail) in details {
        document.note(if conflict { Tone::Warning } else { Tone::Info }, detail);
    }
    let stale = value
        .get("staleRecords")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if stale > 0 {
        document.note(
            Tone::Warning,
            format!("{stale} record(s) from a previous chain instance were ignored."),
        );
    }
    let undeclared: Vec<&str> = value
        .get("undeclared")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !undeclared.is_empty() {
        document.note(
            Tone::Info,
            format!(
                "Recorded but no longer declared in deploy.rs: {}.",
                undeclared.join(", ")
            ),
        );
    }
    if conflicts > 0 {
        document.note(
            Tone::Warning,
            "Deploy refuses while a step is in conflict: restore the step, or name it in --replace.",
        );
    } else if value.get("dryRun").and_then(Value::as_bool) == Some(true) && pending > 0 {
        document.note(
            Tone::Info,
            "Re-run without --dry-run to execute the pending steps.",
        );
    }
    document
}

fn deploy_report_document(value: &Value) -> Document {
    let mut document = Document::default();
    let actions = value
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let executed = actions
        .iter()
        .filter(|a| {
            matches!(
                a.get("outcome").and_then(Value::as_str),
                Some("executed") | Some("redeployed") | Some("adopted")
            )
        })
        .count();
    document.headline(
        Tone::Success,
        format!("Executed {executed} of {} step(s)", actions.len()),
    );
    document.fields(fields(
        value,
        &[("Network", "network"), ("Chain", "chainId")],
    ));
    let mut rows = Vec::new();
    let mut transactions = Vec::new();
    for action in &actions {
        rows.push(vec![
            string_at(action, "kind"),
            string_at(action, "name"),
            string_at(action, "outcome"),
            action
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("-")
                .to_string(),
            action
                .get("detail")
                .and_then(Value::as_str)
                .filter(|detail| detail.chars().count() <= 24)
                .unwrap_or_default()
                .to_string(),
        ]);
        if let Some(txid) = action.get("txid").and_then(Value::as_str) {
            transactions.push(vec![string_at(action, "name"), txid.to_string()]);
        }
    }
    if !rows.is_empty() {
        document.blocks.push(Block::Table(
            vec![
                "Kind".into(),
                "Name".into(),
                "Outcome".into(),
                "Id".into(),
                "Change".into(),
            ],
            rows,
        ));
    }
    // Transaction ids are 64 characters: a table of their own keeps the
    // summary above readable at any width.
    if !transactions.is_empty() {
        document.blocks.push(Block::Table(
            vec!["Step".into(), "Transaction".into()],
            transactions,
        ));
    }
    if actions
        .iter()
        .any(|a| a.get("outcome").and_then(Value::as_str) == Some("redeployed"))
    {
        document.note(
            Tone::Warning,
            "A redeployed contract has a new id and empty storage; the earlier one stays on chain.",
        );
    }
    if executed == 0 {
        document.note(
            Tone::Info,
            "Nothing was pending, so nothing was broadcast. Name a contract or call in --replace to run it again.",
        );
    }
    document
}

fn logs_document(value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    let entries = value.as_array().cloned().unwrap_or_default();
    if entries.is_empty() {
        document.note(Tone::Info, "No log entries found.");
        return document;
    }
    for entry in entries {
        let mut prefix = format!("[{}]", string_at(&entry, "service"));
        if verbose {
            let timestamp = string_at(&entry, "timestamp");
            if timestamp != "0" && !timestamp.is_empty() {
                prefix.push_str(&format!(" [{timestamp}]"));
            }
        }
        document.blocks.push(Block::Text(format!(
            "{prefix} {}",
            string_at(&entry, "message")
        )));
    }
    document
}

fn snapshots_document(value: &Value) -> Document {
    let mut document = Document::default();
    let snapshots = value
        .get("snapshots")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    document.headline(Tone::Info, format!("{} snapshot(s)", snapshots.len()));
    for snapshot in snapshots {
        document
            .blocks
            .push(Block::Text(format!("• {}", value_string(&snapshot))));
    }
    document
}

fn abi_document(command: &str, value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    document.headline(
        Tone::Success,
        if command == "abi-verify" {
            "ABI verified"
        } else {
            "ABI fetched"
        },
    );
    document.fields(fields(
        value,
        &[
            ("Contract", "contract"),
            ("Alkanes ID", "alkanesId"),
            ("Package", "package"),
            ("SHA-256", "abiSha256"),
            ("Local SHA-256", "localAbiSha256"),
            ("Deployed SHA-256", "deployedAbiSha256"),
        ],
    ));
    if verbose {
        if let Some(abi) = value.get("abi") {
            document.blocks.push(Block::Text(
                serde_json::to_string_pretty(abi).unwrap_or_default(),
            ));
        }
    }
    document
}

fn scaffold_document(command: &str, value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    document.headline(
        Tone::Success,
        if command == "init" {
            "Project initialized"
        } else {
            "Contract created"
        },
    );
    document.fields(fields(
        value,
        &[
            ("Project", "project"),
            ("Contract", "contract"),
            ("Directory", "directory"),
        ],
    ));
    if verbose {
        document.fields(fields(value, &[("Files", "files")]));
    }
    if let Some(next) = value.get("next").and_then(Value::as_array) {
        document.note(
            Tone::Info,
            format!(
                "Next: {}",
                next.iter()
                    .map(value_string)
                    .collect::<Vec<_>>()
                    .join("  ·  ")
            ),
        );
    }
    document
}

fn action_or_generic_document(command: &str, value: &Value, verbose: bool) -> Document {
    let mut document = Document::default();
    let action = match command {
        "down" => Some("Labcoat Network stopped"),
        "mine" => Some("Blocks mined"),
        "fund" => Some("Wallet funded"),
        "reset" => Some("Labcoat Network reset"),
        "snapshot" => Some("Snapshot created"),
        "restore" => Some("Snapshot restored"),
        _ => None,
    };
    let fallback = format!("{} succeeded", title(command));
    if action.is_some() {
        document.headline(Tone::Success, action_headline(command, value));
        document.fields(action_fields(command, value));
        if let Some(next) = action_note(command) {
            document.note(Tone::Info, next);
        }
    } else {
        document.headline(Tone::Success, fallback);
        document.blocks.extend(generic_blocks(value));
    }
    if verbose && value.is_object() {
        document.blocks.push(Block::Text(
            serde_json::to_string_pretty(value).unwrap_or_default(),
        ));
    }
    document
}

/// What a Labcoat Network command did, in its own words. The payload also
/// names the network and repeats the action as `true`, which a person
/// running `labcoat down` does not need read back to them.
fn action_headline(command: &str, value: &Value) -> String {
    let count = |key: &str| value.get(key).and_then(Value::as_u64);
    match command {
        "down" => "Labcoat Network stopped".into(),
        "reset" => "Labcoat Network reset".into(),
        "mine" => match count("mined") {
            Some(1) => "Mined 1 block".into(),
            Some(blocks) => format!("Mined {blocks} blocks"),
            None => "Blocks mined".into(),
        },
        "fund" => "Faucet payment sent".into(),
        "snapshot" => format!("Snapshot `{}` created", string_at(value, "snapshot")),
        "restore" => format!("Snapshot `{}` restored", string_at(value, "restored")),
        _ => format!("{} succeeded", title(command)),
    }
}

fn action_fields(command: &str, value: &Value) -> Vec<(String, String)> {
    match command {
        "mine" => fields(value, &[("Height", "height")]),
        "fund" => fields(value, &[("Transaction", "txid")]),
        "snapshot" => fields(value, &[("Path", "path")]),
        _ => Vec::new(),
    }
}

/// What the command left for the developer to do, when that is not obvious.
fn action_note(command: &str) -> Option<&'static str> {
    match command {
        "down" => Some("The chain is kept. `labcoat up` resumes it."),
        "reset" => Some("The chain is gone. `labcoat up` starts a new one."),
        "fund" => Some("Run `labcoat mine 1` to confirm it."),
        "snapshot" | "restore" => Some("Labcoat Network is stopped. `labcoat up` starts it."),
        _ => None,
    }
}

fn balance_document(value: &Value) -> Document {
    let mut document = Document::default();
    document.headline(Tone::Info, "Alkanes balances");
    document.fields(fields(value, &[("Address", "address")]));
    let rows: Vec<Vec<String>> = value
        .get("balances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| {
            let id = entry.get("alkane_id").cloned().unwrap_or(Value::Null);
            vec![
                format!(
                    "{}:{}",
                    value_string(id.get("block").unwrap_or(&Value::Null)),
                    value_string(id.get("tx").unwrap_or(&Value::Null))
                ),
                value_string(entry.get("balance").unwrap_or(&Value::Null)),
            ]
        })
        .collect();
    if rows.is_empty() {
        document.note(Tone::Info, "This address holds no Alkanes.");
    } else {
        document
            .blocks
            .push(Block::Table(vec!["Alkane".into(), "Balance".into()], rows));
    }
    document
}

fn generic_blocks(value: &Value) -> Vec<Block> {
    match value {
        Value::Object(map) => vec![Block::Fields(
            map.iter()
                .map(|(key, value)| (humanize(key), value_string(value)))
                .collect(),
        )],
        Value::Array(items) => items
            .iter()
            .map(|item| Block::Text(format!("• {}", value_string(item))))
            .collect(),
        other => vec![Block::Text(value_string(other))],
    }
}

fn render(document: &Document, width: usize, ansi: bool) -> String {
    let mut output = String::new();
    for (index, block) in document.blocks.iter().enumerate() {
        if index > 0 && !matches!(block, Block::Text(_)) {
            output.push('\n');
        }
        match block {
            Block::Headline(tone, text) => {
                let mark = match tone {
                    Tone::Success => "✓",
                    Tone::Error => "✗",
                    Tone::Warning => "!",
                    _ => "›",
                };
                styled_line(&mut output, *tone, &format!("{mark} {text}"), ansi);
            }
            Block::Fields(fields) => render_fields(&mut output, fields, ansi),
            Block::Table(headers, rows) => render_table(&mut output, headers, rows, width, ansi),
            Block::Tree(lines) => {
                for line in lines {
                    output.push_str(&"  ".repeat(line.depth));
                    output.push_str("• ");
                    output.push_str(&line.summary);
                    output.push('\n');
                }
            }
            Block::Note(tone, text) => styled_line(&mut output, *tone, text, ansi),
            Block::Secret(secret) => {
                styled_line(
                    &mut output,
                    Tone::Warning,
                    "! Recovery phrase — store this securely; it will not be shown again:",
                    ansi,
                );
                output.push_str("  ");
                output.push_str(secret);
                output.push('\n');
            }
            Block::Text(text) => {
                output.push_str(text);
                if !text.ends_with('\n') {
                    output.push('\n');
                }
            }
        }
    }
    output
}

fn render_fields(output: &mut String, fields: &[(String, String)], ansi: bool) {
    let label_width = fields
        .iter()
        .map(|(label, _)| label.width())
        .max()
        .unwrap_or(0);
    for (label, value) in fields {
        if ansi {
            output.push_str(&format!(
                "{}{:label_width$}{}  {value}\n",
                dim(),
                label,
                reset()
            ));
        } else {
            output.push_str(&format!("{label:label_width$}  {value}\n"));
        }
    }
}

fn render_table(
    output: &mut String,
    headers: &[String],
    rows: &[Vec<String>],
    width: usize,
    ansi: bool,
) {
    if rows.is_empty() {
        output.push_str("(none)\n");
        return;
    }
    let columns = headers.len();
    let widths = (0..columns)
        .map(|column| {
            std::iter::once(headers.get(column).map(String::as_str).unwrap_or(""))
                .chain(
                    rows.iter()
                        .map(|row| row.get(column).map(String::as_str).unwrap_or("")),
                )
                .map(UnicodeWidthStr::width)
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let natural = widths.iter().sum::<usize>() + columns.saturating_sub(1) * 2;
    if natural > width && columns > 2 {
        for row in rows {
            for (column, value) in row.iter().enumerate() {
                if value.is_empty() {
                    continue;
                }
                let label = headers.get(column).map(String::as_str).unwrap_or("");
                if ansi {
                    output.push_str(&format!("{}{label}:{} {value}\n", dim(), reset()));
                } else {
                    output.push_str(&format!("{label}: {value}\n"));
                }
            }
            output.push('\n');
        }
        return;
    }
    for (column, header) in headers.iter().enumerate() {
        if ansi {
            output.push_str(&format!("{}{}{}", style(Tone::Info), header, reset()));
        } else {
            output.push_str(header);
        }
        let padding = widths[column].saturating_sub(header.width());
        output.push_str(&" ".repeat(padding));
        if column + 1 < columns {
            output.push_str("  ");
        }
    }
    output.push('\n');
    for row in rows {
        for (column, column_width) in widths.iter().enumerate().take(columns) {
            let value = row.get(column).map(String::as_str).unwrap_or("");
            output.push_str(value);
            let padding = column_width.saturating_sub(value.width());
            output.push_str(&" ".repeat(padding));
            if column + 1 < columns {
                output.push_str("  ");
            }
        }
        output.push('\n');
    }
}

fn styled_line(output: &mut String, tone: Tone, text: &str, ansi: bool) {
    if ansi {
        output.push_str(&format!("{}{text}{}\n", style(tone), reset()));
    } else {
        output.push_str(text);
        output.push('\n');
    }
}

fn style(tone: Tone) -> Style {
    let color = match tone {
        Tone::Success => Some(AnsiColor::Green),
        Tone::Error => Some(AnsiColor::Red),
        Tone::Warning => Some(AnsiColor::Yellow),
        Tone::Info => Some(AnsiColor::Cyan),
    };
    Style::new()
        .fg_color(color.map(Into::into))
        .effects(Effects::BOLD)
}

fn dim() -> Style {
    Style::new().effects(Effects::DIMMED)
}

fn reset() -> &'static str {
    "\u{1b}[0m"
}

fn fields(value: &Value, keys: &[(&str, &str)]) -> Vec<(String, String)> {
    keys.iter()
        .filter_map(|(label, key)| {
            let value = value.get(*key)?;
            if value.is_null() {
                return None;
            }
            Some(((*label).into(), value_string(value)))
        })
        .collect()
}

/// One `Pays vN` field per bitcoin payment output a call carries.
fn payment_fields(value: &Value) -> Vec<(String, String)> {
    value
        .get("payments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|payment| {
            Some((
                format!("Pays {}", payment.get("output")?.as_str()?),
                format!(
                    "{} sats to {}",
                    format_u64(payment.get("sats")?.as_u64()?),
                    payment.get("address")?.as_str()?
                ),
            ))
        })
        .collect()
}

fn replace_field(fields: &mut [(String, String)], label: &str, replacement: String) {
    if let Some((_, value)) = fields.iter_mut().find(|(candidate, _)| candidate == label) {
        *value = replacement;
    }
}

fn value_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .map(value_string)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| format!("{}={}", humanize(key), value_string(value)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn string_at(value: &Value, key: &str) -> String {
    value.get(key).map(value_string).unwrap_or_default()
}

fn title(command: &str) -> String {
    humanize(command)
}

fn humanize(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character == '-' || character == '_' {
            output.push(' ');
        } else if character.is_ascii_uppercase() && index > 0 {
            output.push(' ');
            output.push(character.to_ascii_lowercase());
        } else if index == 0 {
            output.push(character.to_ascii_uppercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn status_mark(status: &str) -> String {
    match status {
        "ok" | "running" | "success" | "installed" => format!("✓ {status}"),
        "warn" | "starting" | "downloading" => format!("! {status}"),
        "fail" | "error" | "stopped" | "revert" => format!("✗ {status}"),
        _ => status.into(),
    }
}

fn binary_status(value: &Value) -> String {
    match value {
        Value::String(status) => status_mark(status),
        Value::Object(status) if status.len() == 1 => {
            let (kind, detail) = status.iter().next().expect("one status variant");
            let detail = detail
                .get("version")
                .or_else(|| detail.get("current"))
                .map(value_string)
                .unwrap_or_default();
            let mark = match kind.as_str() {
                "installed" => "✓",
                "downloading" | "updateavailable" | "update_available" => "!",
                _ => "✗",
            };
            if detail.is_empty() {
                format!("{mark} {}", humanize(kind).to_ascii_lowercase())
            } else {
                format!("{mark} {} ({detail})", humanize(kind).to_ascii_lowercase())
            }
        }
        other => value_string(other),
    }
}

fn format_u64(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn indent(value: &str, depth: usize) -> String {
    let prefix = "  ".repeat(depth);
    value
        .lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_announced_endpoint_never_shows_credentials() {
        assert_eq!(
            display_endpoint("http://127.0.0.1:18443"),
            "http://127.0.0.1:18443"
        );
        assert_eq!(
            display_endpoint("https://user:secret@rpc.example.com/v4/jsonrpc?api_key=abc#frag"),
            "https://rpc.example.com/v4/jsonrpc"
        );
        assert_eq!(display_endpoint("subfrost"), "subfrost");
    }

    #[test]
    fn plan_renders_steps_arguments_and_conflicts_within_the_width() {
        let value = serde_json::json!({
            "script": "deploy.rs",
            "network": "labcoat",
            "height": 108,
            "pending": 2,
            "conflicts": 1,
            "staleRecords": 0,
            "undeclared": ["old_market"],
            "actions": [
                {"kind": "external", "name": "usd", "status": "complete", "action": "none", "id": "4:65012"},
                {"kind": "contract", "name": "counter_logic", "status": "pending", "action": "replace", "target": "1:0",
                 "detail": "the code changed — deploys a new instance and keeps 2:1 in the record's history"},
                {"kind": "contract", "name": "counter_beacon", "status": "conflict", "action": "none", "id": "2:2",
                 "args": ["implementation=2:1", "auth_token_units=2"],
                 "detail": "its definition changed since it ran — restore it, or pass `--replace counter_beacon` to deploy a new instance"},
                {"kind": "setting", "name": "counter_beacon_implementation", "status": "pending", "action": "update", "id": "2:2",
                 "args": ["<counter_logic.id>"], "detail": "2:1 → <counter_logic.id>"},
                {"kind": "contract", "name": "counter", "status": "complete", "action": "none", "id": "2:4",
                 "args": ["beacon=2:2"]},
            ],
        });
        let output = render_plain("plan", &value, false, 100);
        assert!(output.contains("Plan for deploy.rs — 2 pending, 1 in conflict"));
        // The step table stays a table at the default width.
        assert!(output.contains("Kind      Name"));
        assert!(
            output.contains("counter_logic                  pending (replace)  assigned at deploy")
        );
        assert!(!output.contains("1:0"));
        assert!(output.contains("counter_beacon                 conflict           2:2"));
        // Arguments are listed for steps that will run or need a decision…
        assert!(output.contains("implementation=2:1, auth_token_units=2"));
        // …and not for completed ones.
        assert!(!output.contains("beacon=2:2"));
        assert!(output.contains("counter_beacon_implementation: 2:1 → <counter_logic.id>"));
        assert!(output.contains("Recorded but no longer declared in deploy.rs: old_market."));
        assert!(output.contains("Deploy refuses while a step is in conflict"));
    }

    #[test]
    fn deploy_report_lists_transactions_separately() {
        let txid = "73c7d9682d8c881353979b78d60f14f2f18b26221c90bf5f9f7acf35a2ed0b97";
        let value = serde_json::json!({
            "network": "labcoat",
            "actions": [
                {"kind": "contract", "name": "auth_token", "outcome": "skipped-complete", "id": "4:65517"},
                {"kind": "contract", "name": "counter_logic", "outcome": "executed", "id": "2:5", "txid": txid,
                 "detail": "the code changed — deploys a new instance and keeps 2:1 in the record's history"},
                {"kind": "setting", "name": "counter_beacon_implementation", "outcome": "executed", "id": "2:2",
                 "txid": txid, "detail": "2:1 → 2:5"},
            ],
        });
        let output = render_plain("deploy", &value, false, 100);
        assert!(output.contains("Executed 2 of 3 step(s)"));
        assert!(
            output.contains("counter_beacon_implementation  executed          2:2      2:1 → 2:5")
        );
        assert!(output.contains(&format!("counter_logic                  {txid}")));
        // A long explanation never widens the summary table.
        assert!(!output.contains("the code changed"));
        assert!(!output.contains("Nothing was pending"));
    }

    #[test]
    fn deploy_renders_a_plan_a_report_or_a_single_wasm_execution() {
        // --dry-run on deploy.rs steps is the plan, narrowed to a named step.
        let plan = serde_json::json!({
            "script": "deploy.rs", "network": "labcoat", "height": 108, "only": "counter",
            "pending": 1, "conflicts": 0, "staleRecords": 0, "dryRun": true,
            "actions": [{"kind": "contract", "name": "counter", "status": "pending", "action": "create", "target": "1:0"}],
        });
        let output = render_plain("deploy", &plan, false, 100);
        assert!(output.contains("Plan for deploy.rs step `counter` — 1 pending step(s)"));
        assert!(output.contains("Re-run without --dry-run to execute the pending steps."));

        // With nothing left to run says so.
        let report = serde_json::json!({
            "network": "labcoat",
            "actions": [{"kind": "contract", "name": "counter", "outcome": "skipped-complete", "id": "2:4"}],
        });
        let output = render_plain("deploy", &report, false, 100);
        assert!(output.contains("Executed 0 of 1 step(s)"));
        assert!(output.contains("Nothing was pending, so nothing was broadcast."));

        // --wasm is one transaction, rendered like a call.
        let wasm = serde_json::json!({"name": "beta", "status": "success", "alkanesId": "2:9", "txid": "ab"});
        let output = render_plain("deploy", &wasm, false, 100);
        assert!(output.contains("Deploy succeeded"));
        assert!(output.contains("2:9"));
    }

    #[test]
    fn stderr_ansi_follows_the_color_flag_and_terminal_detection() {
        // `always`/`never` never consult the stream.
        assert!(resolve_ansi(ColorMode::Always, || unreachable!()));
        assert!(!resolve_ansi(ColorMode::Never, || unreachable!()));
        // `auto` defers to anstream's detection (TTY, NO_COLOR, CLICOLOR*).
        assert!(!resolve_ansi(ColorMode::Auto, || ColorChoice::Never));
        assert!(resolve_ansi(ColorMode::Auto, || ColorChoice::Always));
        assert!(resolve_ansi(ColorMode::Auto, || ColorChoice::AlwaysAnsi));
    }

    #[test]
    fn state_list_renders_resources_with_chain_identity() {
        let value = serde_json::json!({
            "environment": "default",
            "lineage": "00000000-0000-4000-8000-000000000000",
            "serial": 1,
            "chain": {
                "network": "labcoat",
                "bitcoinNetwork": "regtest",
                "block1Hash": "11".repeat(32),
                "labcoatNetworkInstanceId": "22222222-2222-4222-8222-222222222222"
            },
            "resources": [
                { "address": "contract.counter", "kind": "contract",
                  "activeInstance": "instance-2", "alkanesId": "2:9",
                  "status": "success", "instances": 2 }
            ]
        });
        let output = render_plain("state-list", &value, false, 80);
        assert!(output.contains("Durable state (default)"));
        assert!(output.contains("contract.counter"));
        assert!(output.contains("2:9"));
        assert!(!output.contains("Active instance"));
        assert!(!output.contains("\u{1b}["));

        let verbose = render_plain("state-list", &value, true, 120);
        assert!(verbose.contains("Active instance"));
        assert!(verbose.contains("instance-2"));
    }

    #[test]
    fn state_migrate_reports_backup_and_counts() {
        let value = serde_json::json!({
            "environment": "default",
            "network": "labcoat",
            "statePath": ".labcoat/state/default/state.json",
            "backup": ".labcoat/state/default/backups/labcoat.lock.1755000000000.bak",
            "resources": 3,
            "instances": 3,
            "serial": 1,
            "lineage": "00000000-0000-4000-8000-000000000000",
            "lockfileRegenerated": true
        });
        let output = render_plain("state-migrate", &value, false, 80);
        assert!(output.contains("✓ Durable state created"));
        assert!(output.contains(".labcoat/state/default/state.json"));
        assert!(output.contains("labcoat.lock.1755000000000.bak"));
        assert!(!output.contains("\u{1b}["));
    }

    #[test]
    fn simulation_is_concise_and_formats_gas() {
        let value = serde_json::json!({
            "status":"success",
            "target":"2:8",
            "method":"increment",
            "opcode":"1",
            "decoded":{"uint":"3"},
            "data":"0x03000000000000000000000000000000",
            "gasUsed":52184
        });
        let output = render_plain("simulate", &value, false, 80);
        assert!(output.contains("✓ Simulation succeeded"));
        assert!(output.contains("Result  3"));
        assert!(output.contains("Gas     52,184"));
        assert!(!output.contains("Raw data"));
        assert!(!output.contains("\u{1b}["));
    }

    #[test]
    fn call_decodes_return_without_dumping_trace() {
        let txid = "484e46b2d3db1da1192ae24e62dff7d8ad0f1216f91e9d576b5ce873a073578b";
        let value = serde_json::json!({
            "status":"success", "target":"2:8", "method":"increment", "opcode":"1",
            "fee":368, "txid":txid,
            "traces":[{"trace":[{"type":"return","return_data":"03000000000000000000000000000000","fuel_used":0}]}]
        });
        let output = render_plain("call", &value, false, 60);
        assert!(output.contains(txid));
        assert!(output.contains("Return       3"));
        assert!(!output.contains("fuel used"));
    }

    #[test]
    fn copyable_identifiers_survive_every_supported_test_width() {
        let txid = "484e46b2d3db1da1192ae24e62dff7d8ad0f1216f91e9d576b5ce873a073578b";
        let value = serde_json::json!({"status":"success", "txid":txid, "fee":600});
        for width in [60, 80, 120] {
            let output = render_plain("call", &value, false, width);
            assert!(
                output.contains(txid),
                "identifier was lost at width {width}"
            );
            assert!(!output.contains("\u{1b}["));
        }
    }

    #[test]
    fn verbose_call_includes_trace_tree() {
        let value = serde_json::json!({
            "status":"success", "txid":"abc", "traces":[{"trace":[{"type":"return","return_data":"01"}]}]
        });
        let output = render_plain("call", &value, true, 80);
        assert!(output.contains("return 1"));
    }

    #[test]
    fn a_call_lists_its_payment_outputs() {
        let payments = serde_json::json!([
            {"output": "v1", "address": "bcrt1qpayee", "sats": 10000}
        ]);
        let sent = serde_json::json!({
            "status": "success", "txid": "ab", "traces": null, "payments": payments
        });
        let output = render_plain("call", &sent, false, 100);
        assert!(output.contains("Pays v1"));
        assert!(output.contains("10,000 sats to bcrt1qpayee"));

        let dry = serde_json::json!({"dryRun": true, "method": "mint", "payments": payments});
        let output = render_plain("call", &dry, false, 100);
        assert!(output.contains("10,000 sats to bcrt1qpayee"));
    }

    #[test]
    fn plan_lists_the_payments_a_pending_call_makes() {
        let value = serde_json::json!({
            "script": "deploy.rs", "network": "labcoat", "height": 108,
            "pending": 1, "conflicts": 0, "staleRecords": 0,
            "actions": [
                {"kind": "call", "name": "buy", "status": "pending", "action": "call", "id": "2:4",
                 "payments": ["bcrt1qpayee:10000"]},
                {"kind": "call", "name": "bought", "status": "complete", "action": "none", "id": "2:4",
                 "payments": ["bcrt1qearlier:546"]},
            ],
        });
        let output = render_plain("plan", &value, false, 100);
        assert!(output.contains("Pays"));
        assert!(output.contains("10000 sats to bcrt1qpayee"));
        assert!(!output.contains("bcrt1qearlier"));
    }

    #[test]
    fn an_unconfirmed_call_is_not_reported_as_a_success() {
        let value = serde_json::json!({"status": "unknown", "txid": "ab", "traces": null, "method": "increment"});
        let output = render_plain("call", &value, false, 100);
        assert!(output.contains("Call sent, result not known yet"));
        assert!(output.contains("labcoat trace ab --wait"));
        assert!(!output.contains("succeeded"));
    }

    #[test]
    fn revert_is_visually_explicit_without_color() {
        let value = serde_json::json!({
            "status":"revert", "error":"ALKANES: revert: already initialized",
            "decoded":{}, "gasUsed":0, "data":"0x"
        });
        let output = render_plain("simulate", &value, false, 80);
        assert!(output.contains("✗ Simulation reverted"));
        assert!(output.contains("already initialized"));
    }

    #[test]
    fn wallet_mnemonic_is_always_shown() {
        let value = serde_json::json!({
            "created":true,"network":"labcoat","bitcoinNetwork":"regtest",
            "address":"bcrt1...","walletFile":"wallet.json",
            "mnemonic":"alpha beta gamma"
        });
        let output = render_plain("wallet-init", &value, false, 80);
        assert!(output.contains("Labcoat Network"));
        assert!(output.contains("Bitcoin mode"));
        assert!(output.contains("regtest"));
        assert!(output.contains("alpha beta gamma"));
        assert!(output.contains("will not be shown again"));
    }

    #[test]
    fn status_names_labcoat_network_and_its_regtest_mode() {
        let value = serde_json::json!({
            "network": "labcoat",
            "bitcoin_network": "regtest",
            "services": [],
            "block_height": 101,
            "mempool_size": 0,
            "is_ready": true
        });
        let output = render_plain("status", &value, false, 80);
        assert!(output.contains("Labcoat Network ready"));
        assert!(output.contains("regtest (local/private)"));
    }

    #[test]
    fn network_failures_use_the_new_error_code() {
        let envelope = labcoat_network_envelope("status", &Err("connection refused".to_string()));
        assert_eq!(envelope["error"]["code"], "LABCOAT_NETWORK_ERROR");
        assert!(envelope["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Labcoat Network"));
    }
}
