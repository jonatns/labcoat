//! `labcoat` — the Alkanes toolkit CLI.
//!
//! Labcoat Network verbs (up, down, status, mine, fund, logs, reset, snapshot,
//! restore, binaries) + contract ops (wallet, build, deploy, call,
//! simulate, trace, lock) on the pinned alkanes-rs main commit.

mod contract;
mod deploy_script;
mod docs;
mod doctor;
mod e2e_instance;
mod local_wallet;
mod mcp;
mod output;
mod project;
mod settings;
mod test_command;
mod trace_view;
mod upgrade;

use clap::{CommandFactory, Parser, Subcommand};
use isomer_core::LabcoatNetwork;

#[derive(Parser)]
#[command(
    name = "labcoat",
    version,
    about = "Labcoat is the Rust-native CLI for building, testing, and operating Alkanes smart contracts on Labcoat Network, a managed local Bitcoin regtest."
)]
struct Cli {
    /// Emit a machine-readable JSON envelope on stdout
    #[arg(long, global = true)]
    json: bool,

    /// Show raw data, artifact details, and complete traces in human output
    #[arg(short, long, global = true, conflicts_with = "json")]
    verbose: bool,

    /// Terminal color policy
    #[arg(long, global = true, value_enum, default_value_t)]
    color: output::ColorMode,

    /// Network: labcoat | regtest | signet | testnet | mainnet
    #[arg(long, global = true)]
    network: Option<String>,

    /// JSON-RPC endpoint, or `subfrost` (defaults to Labcoat Network
    /// locally and to Subfrost on signet and mainnet)
    #[arg(long, global = true)]
    rpc_url: Option<String>,

    /// Wallet keystore path (defaults to one per network, outside the project)
    #[arg(long, global = true)]
    wallet_file: Option<String>,

    /// Fee rate in sat/vB for state-changing operations
    #[arg(long, global = true)]
    fee_rate: Option<f32>,

    /// Signing backend: keystore (default) or psbt-file:<dir> for external
    /// PSBT signing
    #[arg(long, global = true)]
    signer: Option<String>,

    /// Durable-state environment (defaults to "default")
    #[arg(long, global = true)]
    environment: Option<String>,

    /// Approve transaction signing without an interactive prompt (public
    /// networks; regtest targets always auto-approve)
    #[arg(long = "yes", global = true)]
    assume_yes: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a Rust-native Labcoat workspace with a Counter starter
    Init {
        /// Project name (prompted for when omitted in an interactive terminal)
        name: Option<String>,
    },
    /// Add a minimal contract package and host integration test to this project
    New {
        /// Contract package name in kebab-case
        name: String,
        /// Vendor a standard upgradeable contract instead of a minimal one:
        /// auth-token, upgradeable, upgradeable-beacon, or beacon-proxy
        #[arg(long)]
        template: Option<String>,
    },
    /// Align this project's alkanes-rs, labcoat-test, and Rust toolchain pins with the installed CLI
    Upgrade {
        /// Show the pins that would change without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Build WASIp1 WebAssembly and run native Rust integration tests
    Test {
        /// Optional Cargo contract package whose host test should run
        /// (with --e2e: a test-name filter instead)
        package: Option<String>,
        /// Run tests/e2e.rs on the project's own e2e network, apart from
        /// the one `labcoat up` starts: start it on a fresh chain, run the
        /// deploy.rs deployment, execute the ignored e2e tests, and stop it
        #[arg(long)]
        e2e: bool,
        /// With --e2e: keep the chain the previous e2e run left instead of
        /// starting a fresh one
        #[arg(long, requires = "e2e")]
        no_reset: bool,
    },
    /// Prepare this CLI release's exact runtime bundle and boot Labcoat Network
    Up {
        /// Skip runtime bundle verification and download
        #[arg(long)]
        no_download: bool,
        /// CI mode: wait (bounded) for full readiness, then print one JSON
        /// envelope with the endpoint manifest; exits non-zero unless the
        /// network and its funded wallet are ready
        #[arg(long)]
        ci: bool,
    },
    /// Stop all Labcoat Network services
    Down,
    /// Show Labcoat Network status (services, block height, mempool)
    Status,
    /// Mine blocks on Labcoat Network
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
        /// Filter to one service (node)
        #[arg(long, value_parser = ["node"])]
        service: Option<String>,
        /// Max entries
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    /// Stop services and wipe all chain/index data
    Reset {
        /// Skip the confirmation prompt (the global --yes also skips it)
        #[arg(short = 'y')]
        yes: bool,
    },
    /// Snapshot the Labcoat Network data directory (stops services first)
    Snapshot {
        name: Option<String>,
        /// List existing snapshots
        #[arg(long)]
        list: bool,
    },
    /// Restore a Labcoat Network snapshot (stops services first)
    Restore { name: String },
    /// Inspect (and with --download, repair) this CLI release's runtime bundle
    Binaries {
        #[arg(long)]
        download: bool,
    },

    /// Wallet management
    #[command(subcommand)]
    Wallet(contract::WalletCmd),
    /// Build Cargo contract packages into build/<package>.{wasm,wasm.gz,abi.json}
    Build {
        /// Optional Cargo package name (omitting it builds every contract)
        package: Option<String>,
        /// Output directory
        #[arg(long, default_value = "build")]
        out_dir: String,
    },
    /// Fetch or verify Wasm-exported contract ABI metadata
    #[command(subcommand)]
    Abi(contract::AbiCmd),
    /// Run deploy.rs and show which deployment steps are pending, complete, or in conflict
    Plan {
        /// Run a step that already ran again: a contract deploys a new
        /// instance, a call executes again (repeatable)
        #[arg(long, value_name = "STEP")]
        replace: Vec<String>,
    },
    /// Execute the pending deployment steps deploy.rs declares, or one of
    /// them by name
    Deploy {
        /// Step name from deploy.rs (every pending step when omitted)
        step: Option<String>,
        /// Show what would happen without signing or broadcasting
        #[arg(long)]
        dry_run: bool,
        /// Run a step that already ran again: a contract deploys a new
        /// instance, a call executes again (repeatable; runs the whole
        /// deployment, so it takes no step name)
        #[arg(long, value_name = "STEP")]
        replace: Vec<String>,
        /// Deploy this raw .wasm artifact outside deploy.rs instead of a
        /// step; it is recorded in labcoat.lock only
        #[arg(long)]
        wasm: Option<String>,
        /// Contract name for --wasm deployments (defaults to file stem)
        #[arg(long)]
        name: Option<String>,
        /// Constructor args for --wasm deployments, one per ABI constructor
        /// parameter (raw u128 / 0x-hex cellpack values when the artifact
        /// exposes no ABI constructor)
        #[arg(long, num_args = 0.., value_delimiter = ',')]
        args: Vec<String>,
        /// Deploy a --wasm artifact to reserved number N (cellpack target
        /// [3,N]) instead of the next free id ([1,0])
        #[arg(long)]
        reserve: Option<u128>,
        #[command(flatten)]
        tx: contract::TxFlags,
    },
    /// Record a contract that already exists on chain as a deployment step, without deploying
    Import {
        /// Contract step name from deploy.rs
        name: String,
        /// The deployed contract's block:tx id
        id: String,
        /// Deployment or reveal transaction ID, used to recover auth tokens and other created IDs
        #[arg(long)]
        txid: Option<String>,
    },
    /// Execute a state-changing call on a deployed contract
    Call {
        /// Contract: labcoat.lock name or block:tx alkanes id
        contract: String,
        /// Exact ABI method name or decimal opcode
        selector: String,
        /// One typed value per ABI parameter, or raw cellpack args for numeric opcodes
        #[arg(num_args = 0..)]
        args: Vec<String>,
        #[command(flatten)]
        tx: contract::TxFlags,
        /// Resolve the method name against this contract's ABI instead
        /// (labcoat.lock name or block:tx id) — for a call through a proxy,
        /// name the implementation
        #[arg(long, value_name = "CONTRACT")]
        abi: Option<String>,
        /// Validate inputs and show what would happen without broadcasting
        #[arg(long)]
        dry_run: bool,
    },
    /// Atomically exchange one wallet's Alkane asset for another wallet's asset
    Exchange {
        /// Asset sold by the seller: labcoat.lock name or block:tx id
        offered: String,
        /// Complete offered quantity delivered to the buyer
        offered_amount: u64,
        /// Asset paid by the buyer: labcoat.lock name or block:tx id
        payment: String,
        /// Complete payment quantity delivered to the seller
        payment_amount: u64,
        /// Seller keystore; --wallet-file is the buyer keystore
        #[arg(long)]
        seller_wallet_file: String,
    },
    /// Build an owner-partitioned exchange plan and unsigned PSBT.
    ExchangePlan {
        /// Asset sold by the seller: labcoat.lock name or block:tx id
        #[arg(required_unless_present = "request", conflicts_with = "request")]
        offered: Option<String>,
        /// Complete offered quantity delivered to the buyer
        #[arg(required_unless_present = "request", conflicts_with = "request")]
        offered_amount: Option<u64>,
        /// Asset paid by the buyer: labcoat.lock name or block:tx id
        #[arg(required_unless_present = "request", conflicts_with = "request")]
        payment: Option<String>,
        /// Complete payment quantity delivered to the seller
        #[arg(required_unless_present = "request", conflicts_with = "request")]
        payment_amount: Option<u64>,
        /// Exchange request file (version 1 JSON, e.g. from a generated web
        /// client); replaces the positional assets and address options
        #[arg(long)]
        request: Option<String>,
        #[arg(long, required_unless_present = "request", conflicts_with = "request")]
        seller_address: Option<String>,
        #[arg(long, required_unless_present = "request", conflicts_with = "request")]
        buyer_address: Option<String>,
        #[arg(long)]
        plan_out: String,
        #[arg(long)]
        psbt_out: String,
    },
    /// Validate a buyer-signed exchange PSBT, sign as seller, and optionally broadcast.
    ExchangeSettle {
        #[arg(long)]
        plan: String,
        #[arg(long)]
        psbt: String,
        #[arg(long)]
        seller_wallet_file: String,
        #[arg(long)]
        broadcast: bool,
    },
    /// Simulate a deployed contract against live indexed chain state
    Simulate {
        /// Contract: labcoat.lock name or block:tx alkanes id
        contract: String,
        /// Exact ABI method name or decimal opcode
        selector: String,
        /// One typed value per ABI parameter, or raw cellpack args for numeric opcodes
        #[arg(num_args = 0..)]
        args: Vec<String>,
        /// Resolve the method name against this contract's ABI instead
        /// (labcoat.lock name or block:tx id) — for a read through a proxy,
        /// name the implementation
        #[arg(long, value_name = "CONTRACT")]
        abi: Option<String>,
    },
    /// Alkanes token balances held by an address
    Balance {
        /// Bitcoin address to query
        address: String,
    },
    /// Decoded protostone traces for a transaction
    Trace {
        txid: String,
        /// Poll until the trace is available
        #[arg(long)]
        wait: bool,
    },
    /// Generate typed application artifacts from labcoat.lock and built ABIs
    #[command(subcommand)]
    Generate(contract::GenerateCmd),
    /// labcoat.lock utilities
    #[command(subcommand)]
    Lock(contract::LockCmd),
    /// Durable deployment state (version-2, per environment)
    #[command(subcommand)]
    State(contract::StateCmd),
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
    /// Serve Labcoat Network in this process; `labcoat up` runs it in the
    /// background
    #[command(hide = true)]
    Node {
        /// Local JSON-RPC port
        #[arg(long)]
        port: u16,
        /// Where the chain is saved
        #[arg(long)]
        data_dir: std::path::PathBuf,
        /// The Alkanes indexer module
        #[arg(long)]
        indexer: std::path::PathBuf,
        /// Where compiled indexer modules are cached
        #[arg(long)]
        cache_dir: std::path::PathBuf,
    },
}

impl Commands {
    fn labcoat_network_command_name(&self) -> Option<&'static str> {
        match self {
            Self::Up { .. } => Some("up"),
            Self::Down => Some("down"),
            Self::Status => Some("status"),
            Self::Mine { .. } => Some("mine"),
            Self::Fund { .. } => Some("fund"),
            Self::Logs { .. } => Some("logs"),
            Self::Reset { .. } => Some("reset"),
            Self::Snapshot { .. } => Some("snapshot"),
            Self::Restore { .. } => Some("restore"),
            Self::Binaries { .. } => Some("binaries"),
            _ => None,
        }
    }
}

#[derive(Subcommand)]
enum McpCmd {
    /// Serve MCP over stdio (newline-delimited JSON-RPC)
    Serve,
}

fn main() {
    let cli = Cli::parse();
    init_tracing(&cli);
    // An e2e run targets its own network through the process environment,
    // which is only safe to change before the runtime starts its threads.
    let e2e_setup = match &cli.command {
        Commands::Test { e2e: true, .. } => {
            let root = project::root();
            if root.join("tests/e2e.rs").is_file() {
                e2e_instance::enter(&root)
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the async runtime");
    let code = runtime.block_on(run(cli, e2e_setup));
    std::process::exit(code);
}

/// Log directives used when `RUST_LOG` is unset. Human output stays quiet
/// apart from warnings; `-v/--verbose` adds Labcoat's own progress logs.
///
/// The upstream Alkanes client warns on every wallet query that Labcoat
/// Network has no batched fetch or ord indexer, and falls back as designed.
/// That is how the local network always works, so by default only its
/// errors are shown; `--verbose` brings the warnings back.
fn default_tracing_directives(verbose: bool) -> &'static str {
    if verbose {
        "warn,labcoat=info,labcoat_core=info,labcoat_test=info,isomer_core=info"
    } else {
        "warn,alkanes_cli_common=error"
    }
}

fn init_tracing(cli: &Cli) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(default_tracing_directives(cli.verbose))
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(output::stderr_ansi(cli.color))
        .init();
}

async fn run(cli: Cli, e2e_setup: Result<(), contract::EnvelopeError>) -> i32 {
    let json = cli.json;
    let output_options = output::Options {
        verbose: cli.verbose,
        color: cli.color,
    };
    if let Err(error) = e2e_setup {
        return output::finish_contract(json, "test", Err(error), output_options);
    }
    // Recovery commands answer inside a project whose settings are broken:
    // they are how the developer finds out what is wrong.
    if let Commands::Docs { llm } = &cli.command {
        let reference = docs::reference(Cli::command(), mcp::tools());
        let _ = llm;
        return if json {
            output::finish(
                true,
                "docs",
                Ok(serde_json::to_value(reference).expect("serializable docs reference")),
                output_options,
            )
        } else {
            println!("{}", reference.render_markdown());
            0
        };
    }
    if let Commands::Doctor = &cli.command {
        let checks = doctor::run().await;
        let failed = checks.iter().any(|c| c.status == "fail");
        output::finish(
            json,
            "doctor",
            Ok(serde_json::json!({ "checks": checks })),
            output_options,
        );
        return if failed { 1 } else { 0 };
    }
    if let Commands::Init { name } = &cli.command {
        let name = match name {
            Some(name) => Ok(name.clone()),
            None if json || !std::io::IsTerminal::is_terminal(&std::io::stdin()) => {
                Err(project::missing_project_name())
            }
            None => {
                let mut stdin = std::io::stdin().lock();
                let mut stderr = std::io::stderr().lock();
                project::prompt_project_name(&mut stdin, &mut stderr)
            }
        };
        return match name {
            Ok(name) => output::finish_contract(json, "init", project::init(&name), output_options),
            Err(error) => output::finish_contract(json, "init", Err(error), output_options),
        };
    }
    if let Commands::New { name, template } = &cli.command {
        return output::finish_contract(
            json,
            "new",
            project::new_contract(name, template.as_deref()),
            output_options,
        );
    }
    if let Commands::Upgrade { dry_run } = &cli.command {
        return output::finish_contract(json, "upgrade", upgrade::run(*dry_run), output_options);
    }
    if let Commands::Node {
        port,
        data_dir,
        indexer,
        cache_dir,
    } = &cli.command
    {
        let options = labcoat_runtime::daemon::Options {
            port: *port,
            data_dir: data_dir.clone(),
            indexer: indexer.clone(),
            cache_dir: cache_dir.clone(),
        };
        // The server is synchronous; run it off the async runtime.
        let served =
            tokio::task::spawn_blocking(move || labcoat_runtime::daemon::run(&options)).await;
        return match served {
            Ok(Ok(())) => 0,
            Ok(Err(message)) => {
                eprintln!("error: {message}");
                1
            }
            Err(error) => {
                eprintln!("error: Labcoat Network stopped unexpectedly: {error}");
                1
            }
        };
    }
    let labcoat_network_command = cli.command.labcoat_network_command_name();
    if let Err(message) = validate_labcoat_network_overrides(&cli) {
        return output::finish_contract(
            json,
            "config",
            Err(contract::EnvelopeError {
                code: "CONFIG_INVALID",
                message,
                hint: "use contract and wallet commands for external network targets",
            }),
            output_options,
        );
    }
    let resolved = match if labcoat_network_command.is_some() {
        Ok(settings::labcoat_network())
    } else {
        settings::resolve(settings::Overrides {
            network: cli.network.as_deref(),
            rpc_url: cli.rpc_url.as_deref(),
            wallet_file: cli.wallet_file.as_deref(),
            fee_rate: cli.fee_rate,
            signer: cli.signer.as_deref(),
            environment: cli.environment.as_deref(),
        })
    } {
        Ok(settings) => settings,
        Err(message) => {
            return output::finish_contract(
                json,
                "config",
                Err(contract::EnvelopeError {
                    code: "CONFIG_INVALID",
                    message,
                    hint: "fix labcoat.toml or override the setting with a CLI flag",
                }),
                output_options,
            )
        }
    };
    let wallet_file = resolved.wallet_file.to_string_lossy();
    let ctx = contract::Ctx::new(
        resolved.network,
        &resolved.rpc_url,
        &wallet_file,
        resolved.fee_rate,
    )
    .with_signer(&resolved.signer)
    .with_assume_yes(cli.assume_yes)
    .with_environment(&resolved.environment)
    .with_color(cli.color);
    // A spinner shares stderr with logs and prompts, and garbles both. With
    // `--verbose` the logs are the progress, and a command that signs on a
    // public network prints what it signs and may ask for approval.
    let spinner = !json && !cli.verbose;
    let signing_spinner = spinner && ctx.config.network.uses_regtest();
    match cli.command {
        Commands::Init { .. } => unreachable!("init handled before configuration loading"),
        Commands::Upgrade { .. } => unreachable!("upgrade handled before configuration loading"),
        Commands::Node { .. } => unreachable!("node handled before configuration loading"),
        Commands::New { .. } => {
            unreachable!("contract scaffolding handled before configuration loading")
        }
        Commands::Test {
            package,
            e2e,
            no_reset,
        } => {
            let progress = output::Progress::new(
                if e2e {
                    "Starting the e2e network, deploying, and running e2e tests…"
                } else {
                    "Building contracts and running tests…"
                },
                spinner,
            );
            let result = if e2e {
                test_command::run_e2e(&ctx, package.as_deref(), no_reset).await
            } else {
                test_command::run(package.as_deref())
            };
            progress.finish();
            output::finish_contract(json, "test", result, output_options)
        }
        Commands::Wallet(cmd) => {
            let (name, res) = contract::wallet(&ctx, cmd, json).await;
            output::finish_contract(json, name, res, output_options)
        }
        Commands::Build { package, out_dir } => {
            let progress = output::Progress::new("Building contract artifacts…", spinner);
            let (cmd_name, res) = contract::build(package.as_deref(), &out_dir);
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Abi(cmd) => {
            let (cmd_name, res) = contract::abi(&ctx, cmd).await;
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Plan { replace } => {
            let progress = output::Progress::new("Planning the deployment…", spinner);
            let (cmd_name, res) = contract::plan(&ctx, &replace).await;
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Deploy {
            step,
            dry_run,
            replace,
            wasm,
            name,
            args,
            reserve,
            tx,
        } => {
            let flags = contract::DeployFlags {
                step,
                replace,
                wasm,
                name,
                args,
                reserve,
                tx,
            };
            let request = match flags.request() {
                Ok(request) => request,
                Err(error) => {
                    return output::finish_contract(json, "deploy", Err(error), output_options)
                }
            };
            if !json && !dry_run {
                ctx.announce_target();
            }
            let progress = output::Progress::new(
                if dry_run {
                    "Planning the deployment…"
                } else {
                    "Deploying…"
                },
                signing_spinner,
            );
            let (cmd_name, res) = contract::deploy(&ctx, request, dry_run).await;
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Import { name, id, txid } => {
            if !json {
                ctx.announce_target();
            }
            let progress = output::Progress::new("Verifying the deployed contract…", spinner);
            let (cmd_name, res) = contract::import(&ctx, &name, &id, txid.as_deref()).await;
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Call {
            contract,
            selector,
            args,
            tx,
            abi,
            dry_run,
        } => {
            if !json && !dry_run {
                ctx.announce_target();
            }
            let progress = output::Progress::new(
                if dry_run {
                    "Validating contract call…"
                } else {
                    "Calling contract…"
                },
                signing_spinner,
            );
            let (cmd_name, res) = if dry_run {
                contract::call_dry_run(&ctx, &contract, &selector, &args, &tx, abi.as_deref()).await
            } else {
                contract::call(&ctx, &contract, &selector, &args, &tx, abi.as_deref()).await
            };
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Exchange {
            offered,
            offered_amount,
            payment,
            payment_amount,
            seller_wallet_file,
        } => {
            if !json {
                ctx.announce_target();
            }
            let progress =
                output::Progress::new("Signing and broadcasting atomic exchange…", signing_spinner);
            let (cmd_name, res) = contract::exchange(
                &ctx,
                &offered,
                offered_amount,
                &payment,
                payment_amount,
                &seller_wallet_file,
            )
            .await;
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::ExchangePlan {
            offered,
            offered_amount,
            payment,
            payment_amount,
            request,
            seller_address,
            buyer_address,
            plan_out,
            psbt_out,
        } => {
            let progress = output::Progress::new("Building exchange plan…", spinner);
            let source = match request {
                Some(path) => contract::ExchangeRequestSource::File { path },
                // Clap enforces required_unless_present, so the flag form is complete here.
                None => contract::ExchangeRequestSource::Flags {
                    offered: offered.expect("clap requires offered"),
                    offered_amount: offered_amount.expect("clap requires offered_amount"),
                    payment: payment.expect("clap requires payment"),
                    payment_amount: payment_amount.expect("clap requires payment_amount"),
                    seller_address: seller_address.expect("clap requires seller_address"),
                    buyer_address: buyer_address.expect("clap requires buyer_address"),
                },
            };
            let (cmd_name, res) = contract::exchange_plan(&ctx, source, &plan_out, &psbt_out).await;
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::ExchangeSettle {
            plan,
            psbt,
            seller_wallet_file,
            broadcast,
        } => {
            if !json && broadcast {
                ctx.announce_target();
            }
            let progress = output::Progress::new(
                if broadcast {
                    "Validating, signing, and broadcasting exchange…"
                } else {
                    "Validating and signing exchange…"
                },
                signing_spinner,
            );
            let (cmd_name, res) =
                contract::exchange_settle(&ctx, &plan, &psbt, &seller_wallet_file, broadcast).await;
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Simulate {
            contract,
            selector,
            args,
            abi,
        } => {
            let (cmd_name, res) =
                contract::simulate(&ctx, &contract, &selector, &args, abi.as_deref()).await;
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Balance { address } => {
            let (cmd_name, res) = contract::balance(&ctx, &address).await;
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Trace { txid, wait } => {
            let progress = output::Progress::new(
                if wait {
                    "Waiting for transaction trace…"
                } else {
                    "Fetching transaction trace…"
                },
                spinner,
            );
            let (cmd_name, res) = contract::trace(&ctx, &txid, wait).await;
            progress.finish();
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Generate(cmd) => {
            let (cmd_name, res) = contract::generate(&ctx, cmd);
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Lock(cmd) => {
            let (cmd_name, res) = contract::lock(cmd);
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::State(cmd) => {
            let (cmd_name, res) = contract::state(&ctx, &resolved.environment, cmd).await;
            output::finish_contract(json, cmd_name, res, output_options)
        }
        Commands::Mcp(McpCmd::Serve) => mcp::serve(ctx).await,
        Commands::Doctor => unreachable!("doctor handled before configuration loading"),
        Commands::Docs { .. } => unreachable!("docs handled before configuration loading"),
        Commands::Up { no_download, ci } => {
            // `--ci` always prints one JSON envelope, and its exit code says
            // whether the network and its funded wallet are ready, so a
            // pipeline can gate on `labcoat up --ci` alone.
            let json = json || ci;
            let finish = |result: Result<serde_json::Value, String>| {
                let failed = result.is_err();
                let code = output::finish(json, "up", result, output_options);
                if ci && failed {
                    1
                } else {
                    code
                }
            };
            let mut network = LabcoatNetwork::new();
            let progress = output::Progress::new("Preparing Labcoat Network…", spinner && !ci);
            if !no_download {
                if let Err(e) = network.ensure_binaries(progress_logger(!json)).await {
                    progress.finish();
                    return finish(Err(e));
                }
            }
            if let Err(e) = network.start() {
                progress.finish();
                return finish(Err(e));
            }
            let mut status = network.status().await;
            if ci {
                // Bounded readiness wait so CI can `labcoat up --ci && test`.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
                while !status.is_ready && std::time::Instant::now() < deadline {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    status = network.status().await;
                }
                if !status.is_ready {
                    let not_ready: Vec<String> = status
                        .services
                        .iter()
                        .filter(|s| s.status != "running")
                        .map(|s| s.id.clone())
                        .collect();
                    std::mem::forget(network);
                    progress.finish();
                    return finish(Err(format!(
                        "Labcoat Network not ready after 120s; still down: {}",
                        not_ready.join(", ")
                    )));
                }
            }
            let wallet = local_wallet::prepare(&mut network).await;
            status = network.status().await;
            let endpoints = network.endpoints();
            // The stack must outlive this process: dropping the handle
            // would stop the children it spawned.
            std::mem::forget(network);
            progress.finish();
            if ci {
                // Interactively a wallet problem is a warning beside a
                // running network. A pipeline's next step needs the wallet.
                if let Some(error) = ci_wallet_error(&wallet) {
                    return finish(Err(error));
                }
            }
            finish(Ok(serde_json::json!({
                "status": status,
                "endpoints": endpoints,
                "wallet": wallet,
            })))
        }
        Commands::Down => {
            let mut network = LabcoatNetwork::new();
            let res = network.stop().map(|_| {
                serde_json::json!({
                    "network": "labcoat",
                    "bitcoin_network": "regtest",
                    "stopped": true
                })
            });
            output::finish(json, "down", res, output_options)
        }
        Commands::Status => {
            let mut network = LabcoatNetwork::new();
            let status = network.status().await;
            output::finish(
                json,
                "status",
                Ok(serde_json::to_value(&status).unwrap()),
                output_options,
            )
        }
        Commands::Mine { count, address } => {
            let network = LabcoatNetwork::new();
            let res = network.mine(count, address).await.map(|height| {
                serde_json::json!({
                    "network": "labcoat",
                    "bitcoin_network": "regtest",
                    "mined": count,
                    "height": height
                })
            });
            output::finish(json, "mine", res, output_options)
        }
        Commands::Fund { address, amount } => {
            let network = LabcoatNetwork::new();
            let res = network.fund(&address, amount).await.map(|txid| {
                serde_json::json!({
                    "network": "labcoat",
                    "bitcoin_network": "regtest",
                    "txid": txid
                })
            });
            output::finish(json, "fund", res, output_options)
        }
        Commands::Logs { service, limit } => {
            let network = LabcoatNetwork::new();
            let logs = network.logs(service, limit);
            output::finish(
                json,
                "logs",
                Ok(serde_json::to_value(&logs).unwrap()),
                output_options,
            )
        }
        Commands::Reset { yes } => {
            if !(yes || cli.assume_yes) && !json {
                eprint!("This wipes all Labcoat Network chain data. Continue? [y/N] ");
                use std::io::BufRead;
                let mut line = String::new();
                let _ = std::io::stdin().lock().read_line(&mut line);
                if !matches!(line.trim(), "y" | "Y" | "yes") {
                    eprintln!("Aborted.");
                    return 1;
                }
            }
            let mut network = LabcoatNetwork::new();
            let res = network.reset().map(|_| {
                serde_json::json!({
                    "network": "labcoat",
                    "bitcoin_network": "regtest",
                    "reset": true
                })
            });
            output::finish(json, "reset", res, output_options)
        }
        Commands::Snapshot { name, list } => {
            let mut network = LabcoatNetwork::new();
            if list || name.is_none() {
                let names = network.snapshots();
                return output::finish(
                    json,
                    "snapshot",
                    Ok(serde_json::json!({
                        "network": "labcoat",
                        "bitcoin_network": "regtest",
                        "snapshots": names
                    })),
                    output_options,
                );
            }
            let name = name.unwrap();
            let res = network.snapshot(&name).map(|path| {
                serde_json::json!({
                    "network": "labcoat",
                    "bitcoin_network": "regtest",
                    "snapshot": name,
                    "path": path
                })
            });
            output::finish(json, "snapshot", res, output_options)
        }
        Commands::Restore { name } => {
            let mut network = LabcoatNetwork::new();
            let res = network.restore(&name).map(|_| {
                serde_json::json!({
                    "network": "labcoat",
                    "bitcoin_network": "regtest",
                    "restored": name
                })
            });
            output::finish(json, "restore", res, output_options)
        }
        Commands::Binaries { download } => {
            let network = LabcoatNetwork::new();
            if download {
                if let Err(e) = network.ensure_binaries(progress_logger(!json)).await {
                    return output::finish(json, "binaries", Err(e), output_options);
                }
            }
            let infos = network.check_binaries();
            output::finish(
                json,
                "binaries",
                Ok(serde_json::to_value(&infos).unwrap()),
                output_options,
            )
        }
    }
}

/// Why `up --ci` cannot report success: the built-in wallet could not be
/// prepared, or holds less than the balance `up` tops it up to.
fn ci_wallet_error(wallet: &serde_json::Value) -> Option<String> {
    if let Some(error) = wallet.get("error").and_then(serde_json::Value::as_str) {
        return Some(format!(
            "Labcoat Network is running, but its built-in wallet is not ready: {error}"
        ));
    }
    let balance = wallet
        .get("balanceSats")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    (balance < local_wallet::MINIMUM_SATS).then(|| {
        format!(
            "Labcoat Network is running, but its built-in wallet holds {balance} sats, less than the {} it needs",
            local_wallet::MINIMUM_SATS
        )
    })
}

fn validate_labcoat_network_overrides(cli: &Cli) -> Result<(), String> {
    let Some(command) = cli.command.labcoat_network_command_name() else {
        return Ok(());
    };
    if let Some(network) = cli.network.as_deref() {
        if !network.eq_ignore_ascii_case("labcoat") {
            return Err(format!(
                "`labcoat {command}` always controls Labcoat Network; remove `--network {network}`"
            ));
        }
    }
    if cli.rpc_url.is_some() {
        return Err(format!(
            "`labcoat {command}` always uses the managed Labcoat Network endpoint; remove `--rpc-url`"
        ));
    }
    Ok(())
}

fn progress_logger(enabled: bool) -> impl Fn(isomer_core::ServiceId, f32) + Send + Clone + 'static {
    move |service, progress| {
        if enabled
            && std::io::IsTerminal::is_terminal(&std::io::stderr())
            && (progress == 0.0 || progress >= 1.0)
        {
            eprintln!("  {} {:.0}%", service.display_name(), progress * 100.0);
        }
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;

    #[test]
    fn up_ci_fails_unless_the_built_in_wallet_is_funded() {
        let ready = serde_json::json!({ "address": "bcrt1p", "balanceSats": 1_000_000_000u64 });
        assert_eq!(ci_wallet_error(&ready), None);

        let failed = serde_json::json!({ "error": "the faucet is empty" });
        assert!(ci_wallet_error(&failed)
            .unwrap()
            .contains("the faucet is empty"));

        let unfunded = serde_json::json!({ "address": "bcrt1p", "balanceSats": 0 });
        assert!(ci_wallet_error(&unfunded).unwrap().contains("holds 0 sats"));
        assert!(ci_wallet_error(&serde_json::json!({})).is_some());
    }

    #[test]
    fn command_tree_passes_clap_validation() {
        // Catches conflicting flags (such as a subcommand reusing a global
        // long name), which otherwise only fail at runtime in debug builds.
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn contract_envelopes_preserve_schema_and_typed_errors() {
        let success = output::contract_envelope("test", &Ok(serde_json::json!({ "passed": true })));
        assert_eq!(success["schema"], "labcoat/v1/test");
        assert_eq!(success["result"]["passed"], true);

        let failure = output::contract_envelope(
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
        assert!(Cli::try_parse_from(["labcoat", "init", "my-alkane"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "init", "--force"]).is_err());
        assert!(Cli::try_parse_from(["labcoat", "init", "--contract", "my-token"]).is_err());
    }

    #[test]
    fn default_tracing_is_quiet_unless_verbose() {
        use tracing_subscriber::EnvFilter;
        let quiet = default_tracing_directives(false);
        assert_eq!(quiet, "warn,alkanes_cli_common=error");
        // `labcoat test` used to print `INFO labcoat_core::compile: building
        // contract packages` on every run.
        assert_eq!(
            EnvFilter::new(quiet).max_level_hint(),
            Some(tracing_subscriber::filter::LevelFilter::WARN)
        );
        let verbose = default_tracing_directives(true);
        assert!(verbose.starts_with("warn,"));
        assert!(verbose.contains("labcoat_core=info"));
        assert_eq!(
            EnvFilter::new(verbose).max_level_hint(),
            Some(tracing_subscriber::filter::LevelFilter::INFO)
        );
    }

    #[test]
    fn human_output_flags_have_the_expected_cli_contract() {
        assert!(Cli::try_parse_from(["labcoat", "--verbose", "status"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "status", "--color", "never"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "tui"]).is_err());
        assert!(Cli::try_parse_from(["labcoat", "--json", "--verbose", "status"]).is_err());
        assert!(Cli::try_parse_from(["labcoat", "--color", "rainbow", "status"]).is_err());
    }

    #[test]
    fn managed_commands_reject_external_network_and_rpc_overrides() {
        let signet = Cli::try_parse_from(["labcoat", "--network", "signet", "up"]).unwrap();
        assert!(validate_labcoat_network_overrides(&signet)
            .unwrap_err()
            .contains("always controls Labcoat Network"));

        let rpc =
            Cli::try_parse_from(["labcoat", "--rpc-url", "http://example", "status"]).unwrap();
        assert!(validate_labcoat_network_overrides(&rpc)
            .unwrap_err()
            .contains("remove `--rpc-url`"));

        let labcoat = Cli::try_parse_from(["labcoat", "--network", "labcoat", "status"]).unwrap();
        assert!(validate_labcoat_network_overrides(&labcoat).is_ok());
    }

    #[test]
    fn state_cli_has_the_expected_shape() {
        assert!(Cli::try_parse_from(["labcoat", "state", "list"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "state", "show", "counter"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "state", "show", "counter", "--history"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "state", "migrate"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "--environment", "dev", "state", "list"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "state"]).is_err());
        assert!(Cli::try_parse_from(["labcoat", "state", "show"]).is_err());
        assert!(Cli::try_parse_from(["labcoat", "state", "forget", "counter"]).is_err());
    }

    #[test]
    fn deploy_cli_leaves_flag_combinations_to_typed_errors() {
        // Whether the flags fit together is checked after parsing, so a
        // mismatch reaches --json callers as an envelope error instead of
        // clap usage text.
        for args in [
            vec!["labcoat", "deploy"],
            vec!["labcoat", "deploy", "counter"],
            vec!["labcoat", "deploy", "--wasm", "/tmp/counter.wasm"],
            vec![
                "labcoat",
                "deploy",
                "counter",
                "--wasm",
                "/tmp/counter.wasm",
            ],
            vec!["labcoat", "deploy", "counter", "--name", "custom"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }

    #[test]
    fn build_accepts_only_optional_package_names() {
        assert!(Cli::try_parse_from(["labcoat", "build"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "build", "counter"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "compile", "counter"]).is_err());
    }

    #[test]
    fn call_and_simulate_accept_method_names_and_numeric_opcodes() {
        assert!(Cli::try_parse_from(["labcoat", "simulate", "counter", "increment"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "simulate", "counter", "1"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "call", "token", "mint", "1000"]).is_ok());
        assert!(Cli::try_parse_from(["labcoat", "call", "counter"]).is_err());
    }

    #[test]
    fn deployment_commands_have_the_expected_shape() {
        let cli = Cli::try_parse_from([
            "labcoat",
            "deploy",
            "--replace",
            "token",
            "--replace",
            "market",
        ])
        .unwrap();
        let Commands::Deploy {
            step,
            replace,
            dry_run,
            ..
        } = cli.command
        else {
            panic!("expected deploy");
        };
        assert_eq!(step, None);
        assert_eq!(replace, ["token", "market"]);
        assert!(!dry_run);

        // A bare name is a deploy.rs step, not a Cargo package.
        let cli = Cli::try_parse_from(["labcoat", "deploy", "counter", "--dry-run"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Deploy { step: Some(step), dry_run: true, .. } if step == "counter"
        ));

        let cli = Cli::try_parse_from(["labcoat", "plan", "--replace", "token"]).unwrap();
        assert!(matches!(cli.command, Commands::Plan { replace } if replace == ["token"]));
        // The script is always the project's deploy.rs.
        assert!(Cli::try_parse_from(["labcoat", "plan", "--manifest", "x.hcl"]).is_err());

        let cli = Cli::try_parse_from(["labcoat", "import", "market", "2:118"]).unwrap();
        assert!(
            matches!(cli.command, Commands::Import { name, id, txid: None } if name == "market" && id == "2:118")
        );
        assert!(Cli::try_parse_from(["labcoat", "import", "market"]).is_err());

        let cli = Cli::try_parse_from([
            "labcoat",
            "new",
            "beacon",
            "--template",
            "upgradeable-beacon",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::New { template: Some(template), .. } if template == "upgradeable-beacon"
        ));
    }

    #[test]
    fn call_and_simulate_resolve_methods_through_a_named_abi() {
        let cli = Cli::try_parse_from([
            "labcoat",
            "call",
            "counter",
            "add",
            "41",
            "--abi",
            "counter_logic",
        ])
        .unwrap();
        let Commands::Call { abi, args, .. } = cli.command else {
            panic!("expected call");
        };
        assert_eq!(abi.as_deref(), Some("counter_logic"));
        assert_eq!(args, ["41"]);

        let cli = Cli::try_parse_from([
            "labcoat",
            "simulate",
            "counter",
            "get_count",
            "--abi",
            "2:5",
        ])
        .unwrap();
        assert!(matches!(cli.command, Commands::Simulate { abi: Some(abi), .. } if abi == "2:5"));
    }

    #[test]
    fn deploy_and_call_accept_transaction_shaping_flags() {
        let cli = Cli::try_parse_from([
            "labcoat",
            "deploy",
            "--wasm",
            "build/token.wasm",
            "--reserve",
            "65011",
            "--args",
            "1,100",
            "--to",
            "bcrt1qexample",
        ])
        .unwrap();
        let Commands::Deploy {
            wasm,
            args,
            reserve,
            tx,
            ..
        } = cli.command
        else {
            panic!("expected deploy");
        };
        assert_eq!(wasm.as_deref(), Some("build/token.wasm"));
        assert_eq!(args, ["1", "100"]);
        assert_eq!(reserve, Some(65_011));
        assert_eq!(tx.to.as_deref(), Some("bcrt1qexample"));

        let cli = Cli::try_parse_from([
            "labcoat",
            "call",
            "series",
            "transfer",
            "--inputs",
            "4:65014:100",
            "--pointer",
            "v1",
            "--edict",
            "4:65014:100:v0",
            "--edict",
            "4:65014:5:v1",
        ])
        .unwrap();
        let Commands::Call { tx, .. } = cli.command else {
            panic!("expected call");
        };
        assert_eq!(tx.inputs.as_deref(), Some("4:65014:100"));
        assert_eq!(tx.pointer.as_deref(), Some("v1"));
        assert_eq!(tx.edicts, vec!["4:65014:100:v0", "4:65014:5:v1"]);

        // Simulate stays read-only: no transaction-shaping flags.
        assert!(
            Cli::try_parse_from(["labcoat", "simulate", "counter", "1", "--inputs", "4:1:1"])
                .is_err()
        );
    }
}
