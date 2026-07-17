//! `labcoat` — the Alkanes toolkit CLI.
//!
//! Devnet verbs (up, down, status, mine, fund, logs, reset, snapshot,
//! restore, binaries) + contract ops (wallet, compile, deploy, call,
//! simulate, trace, lock) on the pinned alkanes-rs develop commit.

mod contract;
mod docs;
mod doctor;
mod mcp;
mod project;
mod settings;
mod test_command;

use clap::{CommandFactory, Parser, Subcommand};
use isomer_core::Devnet;

#[derive(Parser)]
#[command(
    name = "labcoat",
    version,
    about = "Smart contract development toolkit for Alkanes on Bitcoin"
)]
struct Cli {
    /// Emit a machine-readable JSON envelope on stdout
    #[arg(long, global = true)]
    json: bool,

    /// Network: regtest | signet | testnet | mainnet
    #[arg(long, global = true)]
    network: Option<String>,

    /// Unified JSON-RPC endpoint (defaults to the local devnet gateway)
    #[arg(long, global = true)]
    rpc_url: Option<String>,

    /// Wallet keystore path (project-local by default)
    #[arg(long, global = true)]
    wallet_file: Option<String>,

    /// Fee rate in sat/vB for state-changing operations
    #[arg(long, global = true)]
    fee_rate: Option<f32>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a Rust-first Labcoat workspace with a Counter starter
    Init {
        /// Destination directory (defaults to the current directory)
        directory: Option<String>,
        /// Overlay the template onto a non-empty directory
        #[arg(long)]
        force: bool,
    },
    /// Add a minimal contract package and host integration test to this project
    New {
        /// Contract package name in kebab-case
        name: String,
    },
    /// Compile WASIp1 WebAssembly and run native Rust integration tests
    Test {
        /// Optional Cargo contract package whose host test should run
        package: Option<String>,
    },
    /// Download binaries if needed and boot the full devnet stack
    Up {
        /// Skip the binary download/check step
        #[arg(long)]
        no_download: bool,
        /// CI mode: wait (bounded) for full readiness, then emit the
        /// machine-readable endpoint manifest; non-zero exit if the stack
        /// never becomes ready
        #[arg(long)]
        ci: bool,
    },
    /// Stop all devnet services
    Down,
    /// Show devnet status (services, block height, mempool)
    Status,
    /// Mine blocks on the devnet
    Mine {
        /// Number of blocks
        #[arg(default_value_t = 1)]
        count: u32,
        /// Address to mine to (defaults to the dev address)
        #[arg(long)]
        address: Option<String>,
    },
    /// Send BTC from the dev wallet to an address
    Fund {
        address: String,
        /// Amount in BTC
        #[arg(default_value_t = 1.0)]
        amount: f64,
    },
    /// Show recent service logs
    Logs {
        /// Filter to one service (bitcoind, metashrew, ord, esplora, espo, jsonrpc)
        #[arg(long)]
        service: Option<String>,
        /// Max entries
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    /// Stop services and wipe all chain/index data
    Reset {
        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Snapshot the devnet data directory (stops services first)
    Snapshot {
        name: Option<String>,
        /// List existing snapshots
        #[arg(long)]
        list: bool,
    },
    /// Restore a devnet snapshot (stops services first)
    Restore { name: String },
    /// Check (and with --download, fetch) service binaries
    Binaries {
        #[arg(long)]
        download: bool,
    },

    /// Wallet management (keystore at --wallet-file)
    #[command(subcommand)]
    Wallet(contract::WalletCmd),
    /// Compile Cargo contract packages to build/<package>.{wasm,wasm.gz,abi.json}
    Compile {
        /// Optional Cargo package name (omitting it builds every contract)
        package: Option<String>,
        /// Output directory
        #[arg(long, default_value = "build")]
        out_dir: String,
    },
    /// Fetch or verify Wasm-exported contract ABI metadata
    #[command(subcommand)]
    Abi(contract::AbiCmd),
    /// Compile and deploy a contract package, or deploy an explicit raw Wasm
    Deploy {
        /// Exact Cargo contract package name
        #[arg(required_unless_present = "wasm", conflicts_with = "wasm")]
        package: Option<String>,
        /// Explicit path to a raw .wasm artifact (skips compilation)
        #[arg(long, required_unless_present = "package", conflicts_with = "package")]
        wasm: Option<String>,
        /// Contract name for --wasm deployments (defaults to file stem)
        #[arg(long, requires = "wasm", conflicts_with = "package")]
        name: Option<String>,
        /// Constructor cellpack args (u128 / 0x-hex / short strings)
        #[arg(long, num_args = 0.., value_delimiter = ',')]
        args: Vec<String>,
        /// Validate inputs and show what would happen without broadcasting
        #[arg(long)]
        dry_run: bool,
    },
    /// Execute a state-changing call on a deployed contract
    Call {
        /// Contract: labcoat.lock name or block:tx alkanes id
        contract: String,
        /// Opcode number
        opcode: u128,
        /// Cellpack args (u128 / 0x-hex / short strings)
        #[arg(num_args = 0..)]
        args: Vec<String>,
        /// Validate inputs and show what would happen without broadcasting
        #[arg(long)]
        dry_run: bool,
    },
    /// Read-only simulation of a contract call
    Simulate {
        /// Contract: labcoat.lock name or block:tx alkanes id
        contract: String,
        /// Opcode number
        opcode: u128,
        /// Cellpack args (u128 / 0x-hex / short strings)
        #[arg(num_args = 0..)]
        args: Vec<String>,
    },
    /// Decoded protostone traces for a transaction
    Trace {
        txid: String,
        /// Poll until the trace is available
        #[arg(long)]
        wait: bool,
    },
    /// labcoat.lock utilities
    #[command(subcommand)]
    Lock(contract::LockCmd),
    /// Model Context Protocol server (agent integration)
    #[command(subcommand)]
    Mcp(McpCmd),
    /// Print documentation
    Docs {
        /// Emit the full command reference + protocol cheatsheet as one
        /// LLM-ready markdown document
        #[arg(long)]
        llm: bool,
    },
    /// Diagnose the environment (toolchain, ports, binaries, project state)
    Doctor,
}

#[derive(Subcommand)]
enum McpCmd {
    /// Serve MCP over stdio (newline-delimited JSON-RPC)
    Serve,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "isomer_core=warn,labcoat=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let code = run(cli).await;
    std::process::exit(code);
}

async fn run(cli: Cli) -> i32 {
    let json = cli.json;
    if let Commands::Init { directory, force } = &cli.command {
        return finish_scaffold(json, "init", project::init(directory.as_deref(), *force));
    }
    if let Commands::New { name } = &cli.command {
        return finish_scaffold(json, "new", project::new_contract(name));
    }
    let resolved = match settings::resolve(settings::Overrides {
        network: cli.network.as_deref(),
        rpc_url: cli.rpc_url.as_deref(),
        wallet_file: cli.wallet_file.as_deref(),
        fee_rate: cli.fee_rate,
    }) {
        Ok(settings) => settings,
        Err(message) => {
            return finish_contract(
                json,
                "config",
                Err(contract::EnvelopeError {
                    code: "CONFIG_INVALID",
                    message,
                    hint: "fix labcoat.toml or override the setting with a CLI flag",
                }),
            )
        }
    };
    let wallet_file = resolved.wallet_file.to_string_lossy();
    let ctx = contract::Ctx::new(
        &resolved.network,
        &resolved.rpc_url,
        &wallet_file,
        resolved.fee_rate,
    );
    match cli.command {
        Commands::Init { .. } => unreachable!("init handled before configuration loading"),
        Commands::New { .. } => {
            unreachable!("contract scaffolding handled before configuration loading")
        }
        Commands::Test { package } => {
            finish_contract(json, "test", test_command::run(package.as_deref()))
        }
        Commands::Wallet(cmd) => {
            let (name, res) = contract::wallet(&ctx, cmd).await;
            finish_contract(json, name, res)
        }
        Commands::Compile { package, out_dir } => {
            let (cmd_name, res) = contract::compile(package.as_deref(), &out_dir);
            finish_contract(json, cmd_name, res)
        }
        Commands::Abi(cmd) => {
            let (cmd_name, res) = contract::abi(&ctx, cmd).await;
            finish_contract(json, cmd_name, res)
        }
        Commands::Deploy {
            package,
            wasm,
            name,
            args,
            dry_run,
        } => {
            let (cmd_name, res) = if dry_run {
                contract::deploy_dry_run(&ctx, package.as_deref(), wasm.as_deref(), name, &args)
            } else {
                contract::deploy(&ctx, package.as_deref(), wasm.as_deref(), name, &args).await
            };
            finish_contract(json, cmd_name, res)
        }
        Commands::Call {
            contract,
            opcode,
            args,
            dry_run,
        } => {
            let (cmd_name, res) = if dry_run {
                contract::call_dry_run(&ctx, &contract, opcode, &args)
            } else {
                contract::call(&ctx, &contract, opcode, &args).await
            };
            finish_contract(json, cmd_name, res)
        }
        Commands::Simulate {
            contract,
            opcode,
            args,
        } => {
            let (cmd_name, res) = contract::simulate(&ctx, &contract, opcode, &args).await;
            finish_contract(json, cmd_name, res)
        }
        Commands::Trace { txid, wait } => {
            let (cmd_name, res) = contract::trace(&ctx, &txid, wait).await;
            finish_contract(json, cmd_name, res)
        }
        Commands::Lock(cmd) => {
            let (cmd_name, res) = contract::lock(&ctx, cmd);
            finish_contract(json, cmd_name, res)
        }
        Commands::Mcp(McpCmd::Serve) => mcp::serve(ctx).await,
        Commands::Doctor => {
            let checks = doctor::run().await;
            let failed = checks.iter().any(|c| c.status == "fail");
            if json {
                finish(true, "doctor", Ok(serde_json::json!({ "checks": checks })));
            } else {
                for c in &checks {
                    let mark = match c.status {
                        "ok" => "✓",
                        "warn" => "!",
                        _ => "✗",
                    };
                    println!("{} {:<24} {}", mark, c.name, c.detail);
                    if let Some(hint) = &c.hint {
                        println!("    hint: {}", hint);
                    }
                }
            }
            if failed {
                1
            } else {
                0
            }
        }
        Commands::Docs { llm } => {
            let reference = docs::reference(Cli::command(), mcp::tools());
            let _ = llm;
            if json {
                finish(
                    true,
                    "docs",
                    Ok(serde_json::to_value(reference).expect("serializable docs reference")),
                )
            } else {
                println!("{}", reference.render_markdown());
                0
            }
        }
        Commands::Up { no_download, ci } => {
            let mut devnet = Devnet::new();
            if !no_download {
                eprintln!("Checking service binaries...");
                if let Err(e) = devnet.ensure_binaries(progress_logger()).await {
                    return finish(json, "up", Err(e));
                }
            }
            eprintln!("Starting devnet services...");
            if let Err(e) = devnet.start() {
                return finish(json, "up", Err(e));
            }
            let mut status = devnet.status().await;
            if ci {
                // Bounded readiness wait so CI can `labcoat up --ci && test`.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
                while !status.is_ready && std::time::Instant::now() < deadline {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    status = devnet.status().await;
                }
                if !status.is_ready {
                    let not_ready: Vec<String> = status
                        .services
                        .iter()
                        .filter(|s| s.status != "running")
                        .map(|s| s.id.clone())
                        .collect();
                    std::mem::forget(devnet);
                    return finish(
                        json,
                        "up",
                        Err(format!(
                            "devnet not ready after 120s; still down: {}",
                            not_ready.join(", ")
                        )),
                    );
                }
            }
            let endpoints = devnet.endpoints();
            // The stack must outlive this process: dropping the handle
            // would stop the children it spawned.
            std::mem::forget(devnet);
            let payload = serde_json::json!({
                "status": status,
                "endpoints": endpoints,
            });
            if json || ci {
                finish(true, "up", Ok(payload))
            } else {
                println!("Devnet is up.");
                println!(
                    "Unified JSON-RPC: {}",
                    payload["endpoints"]["jsonrpc"].as_str().unwrap_or("?")
                );
                println!("Block height: {}", payload["status"]["block_height"]);
                0
            }
        }
        Commands::Down => {
            let mut devnet = Devnet::new();
            let res = devnet
                .stop()
                .map(|_| serde_json::json!({ "stopped": true }));
            finish(json, "down", res)
        }
        Commands::Status => {
            let mut devnet = Devnet::new();
            let status = devnet.status().await;
            if json {
                finish(json, "status", Ok(serde_json::to_value(&status).unwrap()))
            } else {
                for s in &status.services {
                    println!("{:<22} {:<8} port {}", s.name, s.status, s.port);
                }
                println!("block height: {}", status.block_height);
                println!("mempool: {}", status.mempool_size);
                println!("ready: {}", status.is_ready);
                0
            }
        }
        Commands::Mine { count, address } => {
            let devnet = Devnet::new();
            let res = devnet
                .mine(count, address)
                .await
                .map(|height| serde_json::json!({ "mined": count, "height": height }));
            finish(json, "mine", res)
        }
        Commands::Fund { address, amount } => {
            let devnet = Devnet::new();
            let res = devnet
                .fund(&address, amount)
                .await
                .map(|txid| serde_json::json!({ "txid": txid }));
            finish(json, "fund", res)
        }
        Commands::Logs { service, limit } => {
            let devnet = Devnet::new();
            let logs = devnet.logs(service, limit);
            if json {
                finish(json, "logs", Ok(serde_json::to_value(&logs).unwrap()))
            } else {
                for entry in logs {
                    println!("[{}] {}", entry.service, entry.message);
                }
                0
            }
        }
        Commands::Reset { yes } => {
            if !yes && !json {
                eprint!("This wipes all devnet chain data. Continue? [y/N] ");
                use std::io::BufRead;
                let mut line = String::new();
                let _ = std::io::stdin().lock().read_line(&mut line);
                if !matches!(line.trim(), "y" | "Y" | "yes") {
                    eprintln!("Aborted.");
                    return 1;
                }
            }
            let mut devnet = Devnet::new();
            let res = devnet.reset().map(|_| serde_json::json!({ "reset": true }));
            finish(json, "reset", res)
        }
        Commands::Snapshot { name, list } => {
            let mut devnet = Devnet::new();
            if list || name.is_none() {
                let names = devnet.snapshots();
                return finish(
                    json,
                    "snapshot",
                    Ok(serde_json::json!({ "snapshots": names })),
                );
            }
            let name = name.unwrap();
            let res = devnet
                .snapshot(&name)
                .map(|path| serde_json::json!({ "snapshot": name, "path": path }));
            finish(json, "snapshot", res)
        }
        Commands::Restore { name } => {
            let mut devnet = Devnet::new();
            let res = devnet
                .restore(&name)
                .map(|_| serde_json::json!({ "restored": name }));
            finish(json, "restore", res)
        }
        Commands::Binaries { download } => {
            let devnet = Devnet::new();
            if download {
                if let Err(e) = devnet.ensure_binaries(progress_logger()).await {
                    return finish(json, "binaries", Err(e));
                }
            }
            let infos = devnet.check_binaries();
            if json {
                finish(json, "binaries", Ok(serde_json::to_value(&infos).unwrap()))
            } else {
                for b in infos {
                    println!("{:<24} {:?}  {}", b.service, b.status, b.path);
                }
                0
            }
        }
    }
}

fn progress_logger() -> impl Fn(isomer_core::ServiceId, f32) + Send + Clone + 'static {
    |service, progress| {
        if progress == 0.0 || progress >= 1.0 {
            eprintln!("  {} {:.0}%", service.display_name(), progress * 100.0);
        }
    }
}

/// Envelope printer for contract commands (typed error codes + hints).
fn finish_contract(json: bool, command: &str, res: contract::CmdResult) -> i32 {
    if json {
        let envelope = contract_envelope(command, &res);
        println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
        0
    } else {
        match res {
            Ok(v) => {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                0
            }
            Err(e) => {
                eprintln!("error[{}]: {}", e.code, e.message);
                eprintln!("hint: {}", e.hint);
                1
            }
        }
    }
}

/// Concise terminal output for project scaffolding, while preserving the
/// standard typed JSON envelope for automation.
fn finish_scaffold(json: bool, command: &str, res: contract::CmdResult) -> i32 {
    if json {
        return finish_contract(true, command, res);
    }
    match res {
        Ok(value) => {
            match command {
                "init" => println!(
                    "✓ Initialized {}",
                    value["directory"].as_str().unwrap_or(".")
                ),
                "new" => println!(
                    "✓ Created {}",
                    value["contract"].as_str().unwrap_or("contract")
                ),
                _ => println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_default()
                ),
            }
            0
        }
        Err(error) => {
            eprintln!("error[{}]: {}", error.code, error.message);
            eprintln!("hint: {}", error.hint);
            1
        }
    }
}

fn contract_envelope(command: &str, res: &contract::CmdResult) -> serde_json::Value {
    match res {
        Ok(value) => serde_json::json!({
            "ok": true,
            "command": command,
            "schema": format!("labcoat/v1/{}", command),
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

/// Print the result (JSON envelope or human text) and return the exit code.
fn finish(json: bool, command: &str, res: Result<serde_json::Value, String>) -> i32 {
    if json {
        let envelope = match &res {
            Ok(v) => serde_json::json!({
                "ok": true,
                "command": command,
                "schema": format!("labcoat/v1/{}", command),
                "result": v,
            }),
            Err(e) => serde_json::json!({
                "ok": false,
                "command": command,
                "schema": "labcoat/v1/error",
                "error": {
                    "code": "DEVNET_ERROR",
                    "message": e,
                    "hint": "run `labcoat status` to inspect the devnet"
                },
            }),
        };
        println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
        // An envelope (ok or error) was emitted: exit 0 so callers parse
        // stdout instead of guessing from exit codes.
        0
    } else {
        match res {
            Ok(v) => {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                0
            }
            Err(e) => {
                eprintln!("error: {}", e);
                1
            }
        }
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;

    #[test]
    fn contract_envelopes_preserve_schema_and_typed_errors() {
        let success = contract_envelope("test", &Ok(serde_json::json!({ "passed": true })));
        assert_eq!(success["schema"], "labcoat/v1/test");
        assert_eq!(success["result"]["passed"], true);

        let failure = contract_envelope(
            "test",
            &Err(contract::EnvelopeError {
                code: "TEST_FAILED",
                message: "boom".into(),
                hint: "fix the test",
            }),
        );
        assert_eq!(failure["schema"], "labcoat/v1/error");
        assert_eq!(failure["error"]["code"], "TEST_FAILED");
        assert_eq!(failure["error"]["hint"], "fix the test");
    }

    #[test]
    fn project_scaffolding_cli_has_the_new_top_level_shape() {
        assert!(Cli::try_parse_from(["labcoat", "new", "my-token"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "new"]).is_err());
        assert!(Cli::try_parse_from(["labcoat", "contract", "new", "my-token"]).is_err());
        assert!(Cli::try_parse_from(["labcoat", "init", "--contract", "my-token"]).is_err());
    }

    #[test]
    fn deploy_cli_requires_exactly_one_source() {
        assert!(Cli::try_parse_from(["labcoat", "deploy", "counter"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "deploy", "--wasm", "/tmp/counter.wasm"]).is_ok());
        assert!(Cli::try_parse_from([
            "labcoat",
            "deploy",
            "--wasm",
            "/tmp/counter.wasm",
            "--name",
            "custom"
        ])
        .is_ok());
        assert!(Cli::try_parse_from(["labcoat", "deploy"]).is_err());
        assert!(Cli::try_parse_from([
            "labcoat",
            "deploy",
            "counter",
            "--wasm",
            "/tmp/counter.wasm"
        ])
        .is_err());
        assert!(Cli::try_parse_from(["labcoat", "deploy", "counter", "--name", "custom"]).is_err());
    }
}
