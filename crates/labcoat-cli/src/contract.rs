//! Contract-operation subcommands: thin argument handling over
//! labcoat_core::toolkit, sharing the CLI's JSON envelope conventions.

use clap::Subcommand;
use labcoat_core::{toolkit, ToolkitConfig};
use std::io::Read;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum WalletCmd {
    /// Create (or load) the project wallet. Mnemonic is read from
    /// LABCOAT_MNEMONIC or — with --mnemonic-stdin — from stdin; never argv.
    Init {
        /// Read the mnemonic from stdin (one line)
        #[arg(long)]
        mnemonic_stdin: bool,
    },
    /// Show receive addresses
    Addresses {
        #[arg(long, default_value_t = 1)]
        count: u32,
    },
    /// Show spendable UTXOs
    Utxos,
}

#[derive(Subcommand)]
pub enum LockCmd {
    /// Migrate a legacy deployments/manifest.json into labcoat.lock
    Migrate,
    /// Show the lockfile
    Show,
}

#[derive(Subcommand)]
pub enum AbiCmd {
    /// Fetch ABI metadata from a deployed contract's __meta export
    Fetch {
        /// Contract name from labcoat.lock, or a raw block:tx id
        contract: String,
        /// Write the exact ABI bytes to a file
        #[arg(long)]
        out: Option<String>,
    },
    /// Compare deployed ABI metadata with a locally built contract
    Verify {
        /// Contract name from labcoat.lock, or a raw block:tx id
        contract: String,
        /// Local Cargo contract package (required for raw ids or renamed deployments)
        #[arg(long)]
        package: Option<String>,
    },
}

pub struct Ctx {
    pub config: ToolkitConfig,
}

impl Ctx {
    pub fn new(network: &str, rpc_url: &str, wallet_file: &str, fee_rate: Option<f32>) -> Self {
        Self {
            config: ToolkitConfig {
                network: network.to_string(),
                jsonrpc_url: rpc_url.to_string(),
                wallet_file: PathBuf::from(wallet_file),
                fee_rate: fee_rate.or(Some(2.0)),
            },
        }
    }

    /// Wallet passphrase: env var, with a loud dev default on regtest and a
    /// hard error elsewhere.
    pub fn passphrase(&self) -> Option<String> {
        match std::env::var("LABCOAT_WALLET_PASSPHRASE") {
            Ok(p) if !p.is_empty() => Some(p),
            _ => {
                if self.config.normalized_network() == "regtest" {
                    eprintln!(
                        "warning: LABCOAT_WALLET_PASSPHRASE not set — using the fixed dev passphrase (regtest only)"
                    );
                    Some("labcoat-dev".to_string())
                } else {
                    None
                }
            }
        }
    }
}

fn parse_args(args: &[String]) -> Result<Vec<u128>, labcoat_core::LabcoatError> {
    args.iter().map(|a| labcoat_core::parse_arg(a)).collect()
}

/// Resolve "name-or-id" to (block, tx): block:tx ids parse directly,
/// anything else is looked up in labcoat.lock.
pub(crate) fn resolve(
    config: &ToolkitConfig,
    contract: &str,
) -> Result<(u128, u128), labcoat_core::LabcoatError> {
    if contract.contains(':') {
        toolkit::parse_alkanes_id(contract)
    } else {
        toolkit::resolve_contract(config, contract)
    }
}

fn to_envelope<T: serde::Serialize>(
    r: Result<T, labcoat_core::LabcoatError>,
) -> Result<serde_json::Value, EnvelopeError> {
    match r {
        Ok(v) => Ok(serde_json::to_value(v).expect("serializable")),
        Err(e) => Err(EnvelopeError {
            code: e.code,
            message: e.message,
            hint: e.hint,
        }),
    }
}

#[derive(Debug)]
pub struct EnvelopeError {
    pub code: &'static str,
    pub message: String,
    pub hint: &'static str,
}

pub type CmdResult = Result<serde_json::Value, EnvelopeError>;

pub async fn wallet(ctx: &Ctx, cmd: WalletCmd) -> (&'static str, CmdResult) {
    match cmd {
        WalletCmd::Init { mnemonic_stdin } => {
            let mnemonic = if mnemonic_stdin {
                let mut buf = String::new();
                let _ = std::io::stdin().read_to_string(&mut buf);
                let m = buf.trim().to_string();
                if m.is_empty() {
                    None
                } else {
                    Some(m)
                }
            } else {
                std::env::var("LABCOAT_MNEMONIC")
                    .ok()
                    .filter(|m| !m.is_empty())
            };
            let passphrase = ctx.passphrase();
            let res = async {
                ctx.config.require_passphrase_policy(&passphrase)?;
                let mut provider =
                    labcoat_core::system::connect(&ctx.config, passphrase.clone(), false).await?;
                labcoat_core::wallet::init(&mut provider, &ctx.config, mnemonic, passphrase).await
            }
            .await;
            ("wallet-init", to_envelope(res))
        }
        WalletCmd::Addresses { count } => {
            let res = async {
                let provider =
                    labcoat_core::system::connect(&ctx.config, ctx.passphrase(), true).await?;
                labcoat_core::wallet::addresses(&provider, count).await
            }
            .await;
            ("wallet-addresses", to_envelope(res))
        }
        WalletCmd::Utxos => {
            let res = async {
                let provider =
                    labcoat_core::system::connect(&ctx.config, ctx.passphrase(), true).await?;
                labcoat_core::wallet::utxos(&provider).await
            }
            .await;
            ("wallet-utxos", to_envelope(res))
        }
    }
}

pub fn compile(package: Option<&str>, out_dir: &str) -> (&'static str, CmdResult) {
    let res = (|| {
        let cwd = std::env::current_dir().map_err(|e| {
            labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                e.to_string(),
                "run Labcoat from a Cargo workspace",
            )
        })?;
        let workspace = labcoat_core::workspace::discover(&cwd)?;
        let packages = labcoat_core::workspace::select(&workspace, package)?;
        let outcomes = labcoat_core::compile::compile_packages(
            &workspace,
            &packages,
            &PathBuf::from(out_dir),
            "wasm32-unknown-unknown",
        )?;
        Ok(serde_json::json!({ "contracts": outcomes }))
    })();
    ("compile", to_envelope(res))
}

pub async fn abi(ctx: &Ctx, cmd: AbiCmd) -> (&'static str, CmdResult) {
    match cmd {
        AbiCmd::Fetch { contract, out } => {
            let res = async {
                let (block, tx) = resolve(&ctx.config, &contract)?;
                let bytes = labcoat_core::abi::fetch_deployed(&ctx.config, block, tx).await?;
                if let Some(out) = out {
                    let path = PathBuf::from(out);
                    if let Some(parent) = path
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                    {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            labcoat_core::LabcoatError::new(
                                "TOOLKIT_ERROR",
                                format!("cannot create {}: {e}", parent.display()),
                                "check the output path permissions",
                            )
                        })?;
                    }
                    std::fs::write(&path, &bytes).map_err(|e| {
                        labcoat_core::LabcoatError::new(
                            "TOOLKIT_ERROR",
                            format!("cannot write {}: {e}", path.display()),
                            "check the output path permissions",
                        )
                    })?;
                }
                use sha2::Digest;
                Ok(serde_json::json!({
                    "contract": contract,
                    "alkanesId": format!("{block}:{tx}"),
                    "abi": serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                    "abiSha256": hex::encode(sha2::Sha256::digest(&bytes)),
                }))
            }
            .await;
            ("abi-fetch", to_envelope(res))
        }
        AbiCmd::Verify { contract, package } => {
            let res = async {
                let package = match package {
                    Some(package) => package,
                    None if !contract.contains(':') => contract.clone(),
                    None => {
                        return Err(labcoat_core::LabcoatError::new(
                            "CONFIG_INVALID",
                            "--package is required when verifying a raw contract id",
                            "pass `--package <Cargo package name>`",
                        ))
                    }
                };
                let cwd = std::env::current_dir().map_err(|e| {
                    labcoat_core::LabcoatError::new(
                        "CONFIG_INVALID",
                        e.to_string(),
                        "run Labcoat from a Cargo workspace",
                    )
                })?;
                let workspace = labcoat_core::workspace::discover(&cwd)?;
                labcoat_core::workspace::select(&workspace, Some(&package))?;
                let wasm_path = workspace.root.join("build").join(format!("{package}.wasm"));
                if !wasm_path.is_file() {
                    return Err(labcoat_core::LabcoatError::new(
                        "CONFIG_INVALID",
                        format!("local Wasm not found at {}", wasm_path.display()),
                        "run `labcoat compile <package>` first",
                    ));
                }
                let local = labcoat_core::abi::extract_file(&wasm_path)?;
                let (block, tx) = resolve(&ctx.config, &contract)?;
                let deployed = labcoat_core::abi::fetch_deployed(&ctx.config, block, tx).await?;
                let comparison = labcoat_core::abi::compare(&local, &deployed);
                if !comparison.matches {
                    return Err(labcoat_core::LabcoatError::new(
                        "ABI_MISMATCH",
                        format!(
                            "ABI mismatch for {block}:{tx}: local {}, deployed {}",
                            comparison.local_sha256, comparison.deployed_sha256
                        ),
                        "compile the deployed source revision or verify the target contract id",
                    ));
                }
                Ok(serde_json::json!({
                    "contract": contract,
                    "alkanesId": format!("{block}:{tx}"),
                    "package": package,
                    "matches": true,
                    "localAbiSha256": comparison.local_sha256,
                    "deployedAbiSha256": comparison.deployed_sha256,
                }))
            }
            .await;
            ("abi-verify", to_envelope(res))
        }
    }
}

pub async fn deploy(
    ctx: &Ctx,
    wasm: &str,
    name: Option<String>,
    args: &[String],
) -> (&'static str, CmdResult) {
    let wasm_path = PathBuf::from(wasm);
    let contract_name = name.or_else(|| {
        wasm_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    });
    let res = async {
        let parsed = parse_args(args)?;
        toolkit::deploy(
            &ctx.config,
            ctx.passphrase(),
            &wasm_path,
            contract_name,
            &parsed,
            ctx.config.fee_rate,
        )
        .await
    }
    .await;
    ("deploy", to_envelope(res))
}

/// --dry-run deploy: validate the wasm payload and args, show the plan.
pub fn deploy_dry_run(
    ctx: &Ctx,
    wasm: &str,
    name: Option<String>,
    args: &[String],
) -> (&'static str, CmdResult) {
    let res = (|| {
        let wasm_path = PathBuf::from(wasm);
        let bytes = std::fs::read(&wasm_path).map_err(|e| {
            labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                format!("cannot read {}: {}", wasm_path.display(), e),
                "run `labcoat compile` first",
            )
        })?;
        if bytes.starts_with(&[0x1f, 0x8b]) {
            return Err(labcoat_core::LabcoatError::new(
                "ENVELOPE_INVALID",
                "wasm payload is gzip-compressed; deploy wants the raw .wasm".to_string(),
                "pass the .wasm produced by `labcoat compile`",
            ));
        }
        let parsed = parse_args(args)?;
        use sha2::Digest;
        Ok(serde_json::json!({
            "dryRun": true,
            "network": ctx.config.normalized_network(),
            "wasm": wasm_path.display().to_string(),
            "wasmBytes": bytes.len(),
            "wasmSha256": hex::encode(sha2::Sha256::digest(&bytes)),
            "name": name,
            "cellpackArgs": parsed.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
            "wouldBroadcast": "commit + reveal transactions with the wasm envelope",
        }))
    })();
    ("deploy", to_envelope(res))
}

/// --dry-run call: resolve the contract and args, show the plan.
pub fn call_dry_run(
    ctx: &Ctx,
    contract: &str,
    opcode: u128,
    args: &[String],
) -> (&'static str, CmdResult) {
    let res = (|| {
        let (block, tx) = resolve(&ctx.config, contract)?;
        let parsed = parse_args(args)?;
        Ok(serde_json::json!({
            "dryRun": true,
            "network": ctx.config.normalized_network(),
            "target": format!("{}:{}", block, tx),
            "opcode": opcode.to_string(),
            "cellpackArgs": parsed.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
            "protostoneSpec": labcoat_core::execute::cellpack_spec(block, tx, opcode, &parsed),
            "wouldBroadcast": "one execute transaction carrying the protostone",
        }))
    })();
    ("call", to_envelope(res))
}

pub async fn call(
    ctx: &Ctx,
    contract: &str,
    opcode: u128,
    args: &[String],
) -> (&'static str, CmdResult) {
    let res = async {
        let (block, tx) = resolve(&ctx.config, contract)?;
        let parsed = parse_args(args)?;
        toolkit::call(
            &ctx.config,
            ctx.passphrase(),
            block,
            tx,
            opcode,
            &parsed,
            ctx.config.fee_rate,
        )
        .await
    }
    .await;
    ("call", to_envelope(res))
}

pub async fn simulate(
    ctx: &Ctx,
    contract: &str,
    opcode: u128,
    args: &[String],
) -> (&'static str, CmdResult) {
    let res = async {
        let (block, tx) = resolve(&ctx.config, contract)?;
        let parsed = parse_args(args)?;
        toolkit::simulate(&ctx.config, block, tx, opcode, &parsed).await
    }
    .await;
    ("simulate", to_envelope(res))
}

pub async fn trace(ctx: &Ctx, txid: &str, wait: bool) -> (&'static str, CmdResult) {
    let res = toolkit::trace(&ctx.config, txid, wait)
        .await
        .map(|traces| serde_json::json!({ "txid": txid, "traces": traces }));
    ("trace", to_envelope(res))
}

pub fn lock(ctx: &Ctx, cmd: LockCmd) -> (&'static str, CmdResult) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    match cmd {
        LockCmd::Migrate => {
            let network = ctx.config.normalized_network();
            let res = labcoat_core::lockfile::migrate_legacy(&cwd, &network)
                .map(|n| serde_json::json!({ "migrated": n, "network": network }));
            ("lock-migrate", to_envelope(res))
        }
        LockCmd::Show => {
            let lockfile = labcoat_core::lockfile::load(&cwd);
            (
                "lock-show",
                Ok(serde_json::to_value(lockfile).expect("serializable")),
            )
        }
    }
}
