//! Agent-ready documentation generated from the live Clap and MCP registries.

use clap::Command;
use serde::Serialize;
use serde_json::Value;

const ERROR_CODES: &[(&str, &str, &str)] = &[
    (
        "LABCOAT_NETWORK_ERROR",
        "a Labcoat Network operation failed",
        "run `labcoat status` and inspect `labcoat logs`",
    ),
    (
        "CONFIG_INVALID",
        "configuration is invalid",
        "run `labcoat doctor`",
    ),
    (
        "WALLET_MISSING",
        "the project wallet does not exist",
        "run `labcoat wallet init`",
    ),
    (
        "WALLET_LOCKED",
        "the keystore could not be unlocked",
        "set `LABCOAT_WALLET_PASSPHRASE`",
    ),
    (
        "WALLET_ERROR",
        "wallet metadata, ownership, or signing failed",
        "inspect the wallet, PSBT prevouts, and expected derivation path",
    ),
    (
        "SIGNER_UNSUPPORTED",
        "the selected signer lacks a required capability",
        "use the keystore signer or a compatible PSBT signer",
    ),
    (
        "SIGNER_TIMEOUT",
        "an external signer did not return a PSBT in time",
        "sign the request file or raise `LABCOAT_PSBT_TIMEOUT_SECS`",
    ),
    (
        "SIGNER_MISMATCH",
        "external signer output does not match the requested transaction",
        "sign the exact PSBT without changing inputs or outputs",
    ),
    (
        "EXCHANGE_PLAN_INVALID",
        "exchange terms or fixed output layout are invalid",
        "rebuild the exchange plan from current wallet state",
    ),
    (
        "EXCHANGE_PLAN_MISMATCH",
        "the supplied PSBT differs from its content-addressed plan",
        "use the PSBT emitted by `labcoat exchange-plan`",
    ),
    (
        "EXCHANGE_INPUT_OWNERSHIP",
        "an exchange input is unsafe, ambiguous, or owned by the wrong party",
        "use clean P2TR inputs containing only the participant's required asset",
    ),
    (
        "EXCHANGE_ASSET_UNSAFE",
        "an exchange input or output contains an unrelated or misrouted Alkane",
        "use single-asset owner inputs and rebuild the exchange plan",
    ),
    (
        "EXCHANGE_SELLER_DEBIT",
        "the transaction would consume seller bitcoin value",
        "rebuild with buyer-funded outputs and fees",
    ),
    (
        "EXCHANGE_SIGNATURE_MISSING",
        "a required buyer or seller signature is absent",
        "sign the PSBT with the expected participant wallet",
    ),
    (
        "EXCHANGE_SIGNATURE_INVALID",
        "an exchange input signature failed verification",
        "discard the PSBT and recreate the plan",
    ),
    (
        "EXCHANGE_SIGHASH_UNSUPPORTED",
        "an exchange signature is not Taproot SIGHASH_DEFAULT",
        "sign the complete unchanged transaction with SIGHASH_DEFAULT",
    ),
    (
        "EXCHANGE_NETWORK_MISMATCH",
        "the live chain instance differs from the exchange plan",
        "discard stale plans after a network reset",
    ),
    (
        "EXCHANGE_TIP_STALE",
        "the observed planning tip is no longer in the active chain",
        "rebuild the plan after the reorganization",
    ),
    (
        "EXCHANGE_INPUT_SPENT",
        "a planned input has already been spent",
        "rebuild the plan with current UTXOs",
    ),
    (
        "RPC_UNREACHABLE",
        "the configured Qubitcoin endpoint cannot be reached",
        "run `labcoat status`",
    ),
    (
        "INDEXER_LAG",
        "indexed height did not catch chain height",
        "inspect `qubitcoind` logs",
    ),
    (
        "INSUFFICIENT_FUNDS",
        "spendable BTC cannot cover the operation",
        "fund the wallet and mine a block",
    ),
    (
        "EXECUTION_REVERT",
        "the contract explicitly reverted",
        "inspect the revert reason and trace",
    ),
    (
        "TRACE_TIMEOUT",
        "a decoded trace did not arrive in time",
        "retry `labcoat trace --wait`",
    ),
    (
        "ENVELOPE_INVALID",
        "an Alkanes transaction envelope is invalid",
        "check the contract and arguments",
    ),
    (
        "COMPILE_FAILED",
        "Rust or WebAssembly compilation failed",
        "read stderr and run `labcoat doctor`",
    ),
    (
        "PACKAGE_NOT_FOUND",
        "the requested Cargo contract package was not discovered",
        "run `labcoat build` or pass a package listed in the error",
    ),
    (
        "ABI_MISMATCH",
        "local and deployed __meta output differ",
        "build the deployed source revision and verify the contract ID",
    ),
    (
        "CONTRACT_NOT_FOUND",
        "a contract name or ID could not be resolved",
        "run `labcoat lock show`",
    ),
    (
        "LOCKFILE_INVALID",
        "labcoat.lock exists but cannot be read or parsed",
        "repair the JSON, or delete labcoat.lock to start a fresh ledger",
    ),
    (
        "MANIFEST_INVALID",
        "the alkanes.hcl deployment manifest failed to parse or validate",
        "fix the reported block; references are `alkane.<name>.<field>` / `contract.<name>.<field>`, and conditionals, loops, and functions are not supported",
    ),
    (
        "STATE_INVALID",
        "a .labcoat/state file (the apply call journal or durable environment state) cannot be read or parsed",
        "for the journal: repair or delete the file (calls may re-execute); for durable state: restore backups/state.json.prev",
    ),
    (
        "STATE_MISSING",
        "no version-2 durable state exists for this environment",
        "run `labcoat state migrate`",
    ),
    (
        "STATE_UNSUPPORTED",
        "the durable state schema version is not supported by this labcoat",
        "upgrade labcoat, or restore a backup from .labcoat/state/<environment>/backups",
    ),
    (
        "STATE_LOCKED",
        "another labcoat process holds this environment's lease",
        "wait for the other process; a crashed holder releases the lease automatically",
    ),
    (
        "STATE_CHAIN_MISMATCH",
        "durable state belongs to a different chain instance (e.g. before a `labcoat reset`)",
        "archive .labcoat/state/<environment> or use a different --environment",
    ),
    (
        "STATE_CONFLICT",
        "durable state changed underneath the command, or already exists where none may",
        "re-run against current state (`labcoat state list`)",
    ),
    (
        "APPLY_BLOCKED",
        "an action cannot proceed without manual intervention",
        "read the action's detail in `labcoat plan`",
    ),
    (
        "TOOLKIT_ERROR",
        "the underlying contract toolkit failed",
        "read the error hint",
    ),
    (
        "BINARY_CRASH",
        "a Labcoat Network service exited",
        "inspect `labcoat logs`",
    ),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReference {
    pub version: String,
    pub description: String,
    pub install: String,
    pub core_loop: Vec<String>,
    pub configuration_precedence: Vec<String>,
    pub commands: Vec<CommandReference>,
    pub mcp_protocol_version: String,
    pub mcp_tools: Vec<Value>,
    pub error_codes: Vec<ErrorReference>,
    pub protocol: Vec<ProtocolReference>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReference {
    pub name: String,
    pub path: String,
    pub description: String,
    pub usage: String,
    pub arguments: Vec<ArgumentReference>,
    pub subcommands: Vec<CommandReference>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArgumentReference {
    pub id: String,
    pub description: String,
    pub required: bool,
    pub possible_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorReference {
    pub code: String,
    pub meaning: String,
    pub recovery: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolReference {
    pub name: String,
    pub detail: String,
}

pub fn reference(command: Command, mcp_tools: Vec<Value>) -> AgentReference {
    let commands = command
        .get_subcommands()
        .filter(|subcommand| subcommand.get_name() != "help")
        .map(|subcommand| command_reference(subcommand, "labcoat"))
        .collect();

    AgentReference {
        version: env!("CARGO_PKG_VERSION").to_string(),
        description: "Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts on Labcoat Network, a managed local Bitcoin regtest.".into(),
        install: "curl -fsSL https://labcoat.sh/install | sh".into(),
        core_loop: vec![
            "labcoat init my-project".into(),
            "cd my-project && labcoat test".into(),
            "labcoat up".into(),
            "labcoat wallet init".into(),
            "labcoat fund <address> && labcoat mine 1".into(),
            "labcoat build counter".into(),
            "labcoat deploy counter".into(),
            "labcoat abi verify counter".into(),
            "labcoat simulate counter get_count".into(),
            "labcoat call counter increment".into(),
            "labcoat trace <txid> --wait".into(),
            "labcoat down".into(),
        ],
        configuration_precedence: vec![
            "CLI flags".into(),
            "LABCOAT_* environment variables".into(),
            "labcoat.toml".into(),
            "defaults".into(),
        ],
        commands,
        mcp_protocol_version: crate::mcp::PROTOCOL_VERSION.into(),
        mcp_tools,
        error_codes: ERROR_CODES
            .iter()
            .map(|(code, meaning, recovery)| ErrorReference {
                code: (*code).into(),
                meaning: (*meaning).into(),
                recovery: (*recovery).into(),
            })
            .collect(),
        protocol: vec![
            ProtocolReference {
                name: "Cellpack".into(),
                detail: "[block, tx, opcode, ...args] as u128 values; strings up to 16 bytes are packed little-endian.".into(),
            },
            ProtocolReference {
                name: "Deploy".into(),
                detail: "Targets [1, 0]; raw Wasm is compressed inside a taproot commit/reveal envelope.".into(),
            },
            ProtocolReference {
                name: "Protostone outputs".into(),
                detail: "Trace output for protostone i is transaction.output.len + 1 + i; Labcoat computes it automatically.".into(),
            },
            ProtocolReference {
                name: "Synchronization".into(),
                detail: "State-changing operations wait until the Alkanes index reaches chain height before reading fresh state.".into(),
            },
            ProtocolReference {
                name: "labcoat.lock".into(),
                detail: "Per-network deployment ledger mapping names to Alkanes IDs, Wasm hashes, transaction IDs, and status. Remains the active-address book; `labcoat state migrate` regenerates it from durable state.".into(),
            },
            ProtocolReference {
                name: "Durable state".into(),
                detail: ".labcoat/state/<environment>/state.json is the version-2 per-environment operational state (lineage, serial, chain identity, append-only instance history), created by `labcoat state migrate` and guarded by an OS lease (state.lock). Deploys append instances when it exists and refuse a reset or foreign chain before broadcasting. The flat .labcoat/state/<network>.json apply call journal is separate.".into(),
            },
            ProtocolReference {
                name: "Contract ABI".into(),
                detail: "Named calls use the generated local ABI when its Wasm hash matches labcoat.lock; otherwise they use deployed __meta metadata. Execution always targets deployed code, and numeric opcodes remain the raw cellpack escape hatch.".into(),
            },
            ProtocolReference {
                name: "Generated web client".into(),
                detail: "`labcoat generate web` derives a self-contained TypeScript module tree (manifest, typed ABI descriptors, fetch read client) from labcoat.lock and built ABIs, offline. The client is read-only — indexed height, Alkanes balances, ABI-typed simulate — and holds no keys; browsers reach the unified JSON-RPC endpoint through the app's own dev proxy or rewrite.".into(),
            },
        ],
    }
}

fn command_reference(command: &Command, parent: &str) -> CommandReference {
    let name = command.get_name().to_string();
    let path = format!("{parent} {name}");
    let description = command
        .get_long_about()
        .or_else(|| command.get_about())
        .map(ToString::to_string)
        .unwrap_or_default();
    let mut usage_command = command.clone();
    let usage = usage_command
        .render_usage()
        .to_string()
        .replace("Usage: ", "");
    let arguments = command
        .get_arguments()
        .filter(|argument| {
            let id = argument.get_id().as_str();
            id != "help" && id != "version" && !argument.is_hide_set()
        })
        .map(|argument| ArgumentReference {
            id: argument.get_id().to_string(),
            description: argument
                .get_help()
                .map(ToString::to_string)
                .unwrap_or_default(),
            required: argument.is_required_set(),
            possible_values: argument
                .get_possible_values()
                .iter()
                .map(|value| value.get_name().to_string())
                .collect(),
        })
        .collect();
    let subcommands = command
        .get_subcommands()
        .filter(|subcommand| subcommand.get_name() != "help")
        .map(|subcommand| command_reference(subcommand, &path))
        .collect();

    CommandReference {
        name,
        path,
        description,
        usage,
        arguments,
        subcommands,
    }
}

impl AgentReference {
    pub fn render_markdown(&self) -> String {
        let mut markdown = format!(
            "# Labcoat — command reference & protocol cheatsheet (v{})\n\n{}\n\n",
            self.version, self.description
        );
        markdown.push_str("## Install\n\n```bash\n");
        markdown.push_str(&self.install);
        markdown.push_str("\n```\n\n## The core loop\n\n```bash\n");
        for command in &self.core_loop {
            markdown.push_str(command);
            markdown.push('\n');
        }
        markdown.push_str("```\n\n## Output modes\n\nHuman-readable output is the default, including when stdout is redirected. Add `--verbose` for raw return data, ABI and artifact metadata, and complete traces. `--color auto|always|never` controls styling and `NO_COLOR` is honored.\n\nEvery command accepts `--json` and prints exactly one stable envelope on stdout for agents and automation. Logs and diagnostics go to stderr. When an envelope is printed, inspect its `ok` field instead of the process exit code.\n\n```json\n{\"ok\":true,\"command\":\"status\",\"schema\":\"labcoat/v1/status\",\"result\":{}}\n{\"ok\":false,\"command\":\"deploy\",\"schema\":\"labcoat/v1/error\",\"error\":{\"code\":\"WALLET_MISSING\",\"message\":\"...\",\"hint\":\"run `labcoat wallet init` first\"}}\n```\n\n");
        markdown.push_str("Secrets never ride argv: use `LABCOAT_WALLET_PASSPHRASE`, `LABCOAT_MNEMONIC`, or mnemonic stdin. Configuration precedence is CLI flags → environment → `labcoat.toml` → defaults.\n\n");
        markdown.push_str("## Commands\n\n");
        render_commands(&mut markdown, &self.commands, 3);
        markdown.push_str("## MCP mode\n\n`labcoat mcp serve` exposes the same operations over stdio using MCP protocol version `");
        markdown.push_str(&self.mcp_protocol_version);
        markdown.push_str("`.\n\n| Tool | Description |\n|---|---|\n");
        for tool in &self.mcp_tools {
            let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .replace('|', "\\|");
            markdown.push_str(&format!("| `{name}` | {description} |\n"));
        }
        markdown.push_str("\n## Error codes\n\n| Code | Meaning | Recovery |\n|---|---|---|\n");
        for error in &self.error_codes {
            markdown.push_str(&format!(
                "| `{}` | {} | {} |\n",
                error.code, error.meaning, error.recovery
            ));
        }
        markdown.push_str("\n## Protocol cheatsheet\n\n");
        for note in &self.protocol {
            markdown.push_str(&format!("- **{}**: {}\n", note.name, note.detail));
        }
        markdown.push_str(&format!(
            "\n## alkanes-rs pin\n\nAll alkanes-rs code paths are pinned to commit `{}` on the `main` branch. See TOOLCHAIN.md before changing the pin.\n",
            labcoat_core::compile::ALKANES_RS_REV
        ));
        markdown
    }
}

fn render_commands(markdown: &mut String, commands: &[CommandReference], level: usize) {
    for command in commands {
        markdown.push_str(&format!(
            "{} `{}`\n\n{}\n\n```text\n{}\n```\n\n",
            "#".repeat(level),
            command.path,
            command.description,
            command.usage
        ));
        if !command.arguments.is_empty() {
            markdown.push_str("Arguments and options:\n\n");
            for argument in &command.arguments {
                let required = if argument.required {
                    "required"
                } else {
                    "optional"
                };
                let mut details = Vec::new();
                let description = argument.description.trim();
                if !description.is_empty() {
                    details.push(description.to_owned());
                }
                if !argument.possible_values.is_empty() {
                    details.push(format!(
                        "Values: `{}`.",
                        argument.possible_values.join("`, `")
                    ));
                }
                markdown.push_str(&format!("- `{}` ({required})", argument.id));
                if !details.is_empty() {
                    markdown.push_str(": ");
                    markdown.push_str(&details.join(" "));
                }
                markdown.push('\n');
            }
            markdown.push('\n');
        }
        render_commands(markdown, &command.subcommands, level + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn reference_contains_live_cli_and_mcp_metadata() {
        let reference = reference(crate::Cli::command(), crate::mcp::tools());
        assert!(reference
            .commands
            .iter()
            .any(|command| command.name == "deploy"));
        assert!(reference
            .mcp_tools
            .iter()
            .any(|tool| tool.get("name") == Some(&Value::String("deploy".into()))));
        let markdown = reference.render_markdown();
        assert!(markdown.contains("command reference"));
        assert!(!markdown
            .lines()
            .any(|line| line.ends_with(' ') || line.ends_with('\t')));
    }
}
