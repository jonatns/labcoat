//! `labcoat` — the Alkanes toolkit CLI.
//!
//! Devnet verbs (Phase 2): up, down, status, mine, fund, logs, reset,
//! snapshot, restore, binaries. Contract ops arrive with labcoat-core.

use clap::{Parser, Subcommand};
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

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download binaries if needed and boot the full devnet stack
    Up {
        /// Skip the binary download/check step
        #[arg(long)]
        no_download: bool,
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
    match cli.command {
        Commands::Up { no_download } => {
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
            let status = devnet.status().await;
            let endpoints = devnet.endpoints();
            // The stack must outlive this process: dropping the handle
            // would stop the children it spawned.
            std::mem::forget(devnet);
            let payload = serde_json::json!({
                "status": status,
                "endpoints": endpoints,
            });
            if json {
                finish(json, "up", Ok(payload))
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
            let res = devnet.stop().map(|_| serde_json::json!({ "stopped": true }));
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
