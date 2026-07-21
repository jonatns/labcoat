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
        let envelope = match &result {
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
                    "code": "DEVNET_ERROR",
                    "message": error,
                    "hint": "run `labcoat status` to inspect the devnet"
                },
            }),
        };
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
        crossterm::terminal::size()
            .map(|(width, _)| usize::from(width))
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

fn color_choice(mode: ColorMode) -> ColorChoice {
    match mode {
        ColorMode::Auto => ColorChoice::Auto,
        ColorMode::Always => ColorChoice::Always,
        ColorMode::Never => ColorChoice::Never,
    }
}

fn build_document(command: &str, value: &Value, verbose: bool) -> Document {
    match command {
        "call" | "deploy" => execution_document(command, value, verbose),
        "simulate" => simulate_document(value, verbose),
        "trace" => trace_document(value, verbose),
        "status" | "up" => status_document(command, value, verbose),
        "doctor" => doctor_document(value),
        "binaries" => binaries_document(value, verbose),
        "wallet-addresses" => wallet_addresses_document(value),
        "wallet-utxos" => wallet_utxos_document(value, verbose),
        "wallet-init" => wallet_init_document(value),
        "build" | "test" => build_document_report(command, value, verbose),
        "lock-show" => lock_document(value, verbose),
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
                ("Would broadcast", "wouldBroadcast"),
            ],
        ));
        return document;
    }

    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("success");
    let reverted = status == "revert";
    document.headline(
        if reverted { Tone::Error } else { Tone::Success },
        format!(
            "{} {}",
            title(command),
            if reverted { "reverted" } else { "succeeded" }
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
            "Devnet is ready"
        } else if ready {
            "Devnet ready"
        } else {
            "Devnet not ready"
        },
    );
    document.fields(vec![
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
    } else if let Some(endpoint) = value.pointer("/endpoints/jsonrpc").and_then(Value::as_str) {
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
    let failed = checks
        .iter()
        .any(|check| check.get("status").and_then(Value::as_str) == Some("fail"));
    document.headline(
        if failed { Tone::Error } else { Tone::Success },
        if failed {
            "Environment has problems"
        } else {
            "Environment looks good"
        },
    );
    let rows = checks
        .into_iter()
        .map(|check| {
            vec![
                status_mark(
                    check
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown"),
                ),
                string_at(&check, "name"),
                string_at(&check, "detail"),
                string_at(&check, "hint"),
            ]
        })
        .collect();
    document.blocks.push(Block::Table(
        vec!["".into(), "Check".into(), "Detail".into(), "Hint".into()],
        rows,
    ));
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
    document.fields(fields(
        value,
        &[
            ("Network", "network"),
            ("Address", "address"),
            ("Wallet file", "walletFile"),
        ],
    ));
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
        "down" => Some("Devnet stopped"),
        "mine" => Some("Blocks mined"),
        "fund" => Some("Wallet funded"),
        "reset" => Some("Devnet reset"),
        "snapshot" => Some("Snapshot created"),
        "restore" => Some("Snapshot restored"),
        "lock-migrate" => Some("Lockfile migrated"),
        _ => None,
    };
    let fallback = format!("{} succeeded", title(command));
    document.headline(Tone::Success, action.unwrap_or(&fallback));
    document.blocks.extend(generic_blocks(value));
    if verbose && value.is_object() {
        document.blocks.push(Block::Text(
            serde_json::to_string_pretty(value).unwrap_or_default(),
        ));
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
        if index > 0 && (digits.len() - index) % 3 == 0 {
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
            "created":true,"network":"regtest","address":"bcrt1...","walletFile":"wallet.json",
            "mnemonic":"alpha beta gamma"
        });
        let output = render_plain("wallet-init", &value, false, 80);
        assert!(output.contains("alpha beta gamma"));
        assert!(output.contains("will not be shown again"));
    }
}
