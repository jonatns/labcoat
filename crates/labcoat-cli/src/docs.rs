//! Agent-ready documentation generated from the live Clap and MCP registries.

use clap::Command;
use serde::Serialize;
use serde_json::Value;

const ERROR_CODES: &[(&str, &str, &str)] = &[
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
        "RPC_UNREACHABLE",
        "the configured gateway cannot be reached",
        "run `labcoat status`",
    ),
    (
        "INDEXER_LAG",
        "indexed height did not catch chain height",
        "inspect metashrew logs",
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
        "TOOLKIT_ERROR",
        "the underlying contract toolkit failed",
        "read the error hint",
    ),
    (
        "BINARY_CRASH",
        "a managed devnet service exited",
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
        description: "Rust-native toolkit for building, testing, deploying, and operating Alkanes smart contracts on Bitcoin.".into(),
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
            "labcoat call counter <opcode> [args...]".into(),
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
                detail: "Per-network deployment ledger mapping names to Alkanes IDs, Wasm hashes, transaction IDs, and status.".into(),
            },
            ProtocolReference {
                name: "Contract ABI".into(),
                detail: "Compile and test execute the Wasm __meta export locally; abi fetch and abi verify use Metashrew only for explicit deployed-bytecode inspection.".into(),
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
        markdown.push_str("```\n\n## JSON envelopes (agent mode)\n\nEvery command accepts `--json` and prints exactly one envelope on stdout. Logs go to stderr. When an envelope is printed, inspect its `ok` field instead of the process exit code.\n\n```json\n{\"ok\":true,\"command\":\"status\",\"schema\":\"labcoat/v1/status\",\"result\":{}}\n{\"ok\":false,\"command\":\"deploy\",\"schema\":\"labcoat/v1/error\",\"error\":{\"code\":\"WALLET_MISSING\",\"message\":\"...\",\"hint\":\"run `labcoat wallet init` first\"}}\n```\n\n");
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
            "\n## alkanes-rs pin\n\nAll alkanes-rs code paths are pinned to commit `{}` on the `develop` branch. See TOOLCHAIN.md before changing the pin.\n",
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
