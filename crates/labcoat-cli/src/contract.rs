//! Contract-operation subcommands: thin argument handling over
//! labcoat_core::toolkit, sharing the CLI's JSON envelope conventions.

use clap::Subcommand;
use labcoat_core::{toolkit, NetworkTarget, ToolkitConfig};
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
        /// Include a freshly generated mnemonic in machine-readable output
        /// (--json / MCP). Interactive terminal output always shows it —
        /// that is the one chance to write it down.
        #[arg(long)]
        show_mnemonic: bool,
    },
    /// Show receive addresses
    Addresses {
        #[arg(long, default_value_t = 1)]
        count: u32,
    },
    /// Show spendable UTXOs
    Utxos,
    /// Sign a PSBT file with this wallet's keys (the offline half of the
    /// psbt-file signer workflow)
    SignPsbt {
        /// Unsigned PSBT file (base64 or hex)
        #[arg(long = "in")]
        input: String,
        /// Output path (defaults to `<in stem>.signed.psbt`)
        #[arg(long = "out")]
        output: Option<String>,
    },
    /// Sign a 32-byte application digest with the tweaked key controlling an
    /// owned P2TR address.
    SignDigest {
        #[arg(long)]
        address: String,
        /// Exactly 32 bytes as lowercase or uppercase hexadecimal.
        #[arg(long)]
        digest: String,
    },
}

#[derive(Subcommand)]
pub enum LockCmd {
    /// Show the lockfile
    Show,
}

#[derive(Subcommand)]
pub enum GenerateCmd {
    /// Emit a self-contained TypeScript browser read client: network
    /// manifest, typed ABI descriptors, and a fetch-based client for
    /// indexed height, Alkanes balances, and ABI-typed simulate calls.
    /// Reads labcoat.lock and built ABIs only — no network access.
    Web {
        /// Output directory for the generated TypeScript module tree
        #[arg(long, default_value = "generated/web")]
        out_dir: String,
        /// Directory containing <package>.abi.json build artifacts
        #[arg(long, default_value = "build")]
        build_dir: String,
    },
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
    color: crate::output::ColorMode,
    /// Signing backend spec string (`keystore` or `psbt-file:<dir>`).
    signer: String,
}

impl Ctx {
    pub fn new(
        network: NetworkTarget,
        rpc_url: &str,
        wallet_file: &str,
        fee_rate: Option<f32>,
    ) -> Self {
        Self {
            config: ToolkitConfig {
                network,
                rpc_url: rpc_url.to_string(),
                wallet_file: PathBuf::from(wallet_file),
                fee_rate: fee_rate.or(Some(2.0)),
                assume_yes: false,
            },
            color: crate::output::ColorMode::Auto,
            signer: "keystore".to_string(),
        }
    }

    pub fn with_color(mut self, color: crate::output::ColorMode) -> Self {
        self.color = color;
        self
    }

    pub fn with_signer(mut self, signer: &str) -> Self {
        self.signer = signer.to_string();
        self
    }

    pub fn with_assume_yes(mut self, assume_yes: bool) -> Self {
        self.config.assume_yes = assume_yes;
        self
    }

    fn warn(&self, message: &str) {
        crate::output::print_warning(message, self.color);
    }

    /// Wallet passphrase: env var first; on regtest a loud dev default; on
    /// public networks an interactive prompt when a terminal is available
    /// (CI keeps using the env var).
    pub fn passphrase(&self) -> Option<String> {
        match std::env::var("LABCOAT_WALLET_PASSPHRASE") {
            Ok(p) if !p.is_empty() => Some(p),
            _ => {
                if self.config.network.uses_regtest() {
                    self.warn("LABCOAT_WALLET_PASSPHRASE not set — using the fixed development passphrase (Labcoat Network and custom regtest only)");
                    Some("labcoat-dev".to_string())
                } else if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                    rpassword::prompt_password(format!(
                        "wallet passphrase ({}): ",
                        self.config.network_id()
                    ))
                    .ok()
                    .filter(|p| !p.is_empty())
                } else {
                    None
                }
            }
        }
    }

    /// The configured signing backend. Both backends carry the passphrase:
    /// the keystore signs with it; psbt-file only unlocks for upstream's
    /// mnemonic-derived PSBT metadata (signatures stay external).
    pub fn signer_spec(
        &self,
    ) -> Result<labcoat_core::signer::SignerSpec, labcoat_core::LabcoatError> {
        labcoat_core::signer::SignerSpec::parse(&self.signer, self.passphrase())
    }

    /// Provider with the keystore loaded for read paths (addresses, UTXOs).
    pub async fn wallet_provider(
        &self,
    ) -> Result<labcoat_core::ConcreteProvider, labcoat_core::LabcoatError> {
        let (labcoat_core::signer::SignerSpec::Keystore { passphrase }
        | labcoat_core::signer::SignerSpec::PsbtFile { passphrase, .. }) = self.signer_spec()?;
        labcoat_core::system::connect(&self.config, passphrase, true).await
    }
}

fn parse_args(args: &[String]) -> Result<Vec<u128>, labcoat_core::LabcoatError> {
    args.iter().map(|a| labcoat_core::parse_arg(a)).collect()
}

/// Transaction-shaping flags shared by `deploy` and `call`.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct TxFlags {
    /// Extra transaction inputs: comma-separated alkanes `block:tx:amount`
    /// (amount 0 means all) or bitcoin `B:sats`
    #[arg(long)]
    pub inputs: Option<String>,
    /// Recipient address for the protostone outputs (defaults to the
    /// wallet's primary address)
    #[arg(long)]
    pub to: Option<String>,
    /// Protostone pointer target: vN (physical output) or pN (protostone)
    #[arg(long)]
    pub pointer: Option<String>,
    /// Protostone refund target (defaults to the pointer target)
    #[arg(long)]
    pub refund: Option<String>,
    /// Edict `block:tx:amount:target` appended to the protostone (repeatable)
    #[arg(long = "edict")]
    pub edicts: Vec<String>,
}

impl TxFlags {
    fn options(&self) -> labcoat_core::execute::TxOptions {
        labcoat_core::execute::TxOptions {
            inputs: self.inputs.clone(),
            to: self.to.clone(),
            pointer: self.pointer.clone(),
            refund: self.refund.clone(),
            edicts: self.edicts.clone(),
        }
    }

    /// Non-default fields as dry-run metadata.
    fn metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "inputs": self.inputs,
            "to": self.to,
            "pointer": self.pointer,
            "refund": self.refund,
            "edicts": self.edicts,
        })
    }
}

fn deploy_target(reserve: Option<u128>) -> toolkit::DeployTarget {
    match reserve {
        Some(n) => toolkit::DeployTarget::Reserve(n),
        None => toolkit::DeployTarget::New,
    }
}

/// Encode deploy constructor args via the artifact's ABI, surfacing the
/// core helper's fallback note as a CLI warning.
fn resolve_constructor_args(
    ctx: &Ctx,
    wasm_path: &std::path::Path,
    args: &[String],
) -> Result<labcoat_core::abi::EncodedConstructor, labcoat_core::LabcoatError> {
    let constructor = labcoat_core::abi::encode_constructor(
        wasm_path,
        &labcoat_core::abi::CallArgs::Positional(args.to_vec()),
    )?;
    if let Some(note) = &constructor.note {
        ctx.warn(note);
    }
    Ok(constructor)
}

#[derive(Debug, PartialEq, Eq)]
struct ResolvedInvocation {
    opcode: u128,
    method: Option<String>,
    cellpack_args: Vec<u128>,
    target: String,
    abi_source: Option<AbiSource>,
    local_build_status: LocalBuildStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum AbiSource {
    LocalBuild,
    DeployedMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum LocalBuildStatus {
    Matches,
    Differs,
    Unavailable,
}

impl ResolvedInvocation {
    fn metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "target": self.target,
            "targetRevision": "deployed",
            "abiSource": self.abi_source,
            "localBuildStatus": self.local_build_status,
            "method": self.method,
            "opcode": self.opcode.to_string(),
        })
    }

    fn enrich(&self, value: serde_json::Value) -> serde_json::Value {
        let mut value = match value {
            serde_json::Value::Object(object) => object,
            other => {
                let mut object = serde_json::Map::new();
                object.insert("value".into(), other);
                object
            }
        };
        let serde_json::Value::Object(metadata) = self.metadata() else {
            unreachable!("invocation metadata is always an object")
        };
        value.extend(metadata);
        serde_json::Value::Object(value)
    }
}

fn numeric_selector(selector: &str) -> Result<Option<u128>, labcoat_core::LabcoatError> {
    if selector.is_empty() {
        return Err(labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            "contract call selector must not be empty",
            "pass an ABI method name or a numeric opcode",
        ));
    }
    if let Ok(opcode) = selector.parse::<u128>() {
        return Ok(Some(opcode));
    }
    if selector.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            format!("numeric opcode `{selector}` does not fit in u128"),
            "pass a decimal opcode that fits in u128",
        ));
    }
    Ok(None)
}

async fn resolve_invocation(
    ctx: &Ctx,
    contract: &str,
    selector: &str,
    args: &[String],
    operation: &str,
) -> Result<(u128, u128, ResolvedInvocation), labcoat_core::LabcoatError> {
    let numeric = numeric_selector(selector)?;
    let (block, tx) = resolve(&ctx.config, contract)?;
    let target = format!("{block}:{tx}");
    let deployment = named_deployment(&ctx.config, contract)?;
    let mut local_build_status = deployment
        .as_ref()
        .map(|deployment| local_build_status(contract, deployment))
        .unwrap_or(LocalBuildStatus::Unavailable);
    if local_build_status == LocalBuildStatus::Differs {
        if numeric.is_some() {
            ctx.warn(&format!(
                "local build for {contract} differs from deployed {target}; {operation} is targeting the deployed code. The numeric selector bypasses ABI lookup. Run `labcoat deploy {contract}` to update it."
            ));
        } else {
            ctx.warn(&format!(
                "local build for {contract} differs from deployed {target}; {operation} is using the deployed code and ABI. Run `labcoat deploy {contract}` to update it."
            ));
        }
    }
    let invocation = if let Some(opcode) = numeric {
        ResolvedInvocation {
            opcode,
            method: None,
            cellpack_args: parse_args(args)?,
            target,
            abi_source: None,
            local_build_status,
        }
    } else {
        let local_abi = if local_build_status == LocalBuildStatus::Matches {
            read_local_abi(contract)
        } else {
            None
        };
        let (abi, abi_source) = if let Some(abi) = local_abi {
            (abi, AbiSource::LocalBuild)
        } else {
            if local_build_status == LocalBuildStatus::Matches {
                local_build_status = LocalBuildStatus::Unavailable;
            }
            (
                labcoat_core::abi::fetch_deployed(&ctx.config, block, tx).await?,
                AbiSource::DeployedMeta,
            )
        };
        let method = labcoat_core::abi::resolve_method(
            &abi,
            selector,
            &labcoat_core::abi::CallArgs::Positional(args.to_vec()),
        )?;
        ResolvedInvocation {
            opcode: method.opcode,
            method: Some(method.name),
            cellpack_args: method.cellpack_args,
            target,
            abi_source: Some(abi_source),
            local_build_status,
        }
    };
    Ok((block, tx, invocation))
}

fn named_deployment(
    config: &ToolkitConfig,
    contract: &str,
) -> Result<Option<labcoat_core::lockfile::Deployment>, labcoat_core::LabcoatError> {
    if contract.contains(':') {
        return Ok(None);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    labcoat_core::lockfile::get(&cwd, config.network_id(), contract)
}

fn local_build_status(
    contract: &str,
    deployment: &labcoat_core::lockfile::Deployment,
) -> LocalBuildStatus {
    let Some(expected_hash) = deployment.wasm_sha256.as_deref() else {
        return LocalBuildStatus::Unavailable;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    local_build_status_in(&cwd, contract, expected_hash)
}

fn local_build_status_in(
    root: &std::path::Path,
    contract: &str,
    expected_hash: &str,
) -> LocalBuildStatus {
    let wasm_path = root.join("build").join(format!("{contract}.wasm"));
    let Ok(wasm) = std::fs::read(wasm_path) else {
        return LocalBuildStatus::Unavailable;
    };
    use sha2::Digest;
    let actual_hash = hex::encode(sha2::Sha256::digest(&wasm));
    if actual_hash.eq_ignore_ascii_case(expected_hash) {
        LocalBuildStatus::Matches
    } else {
        LocalBuildStatus::Differs
    }
}

fn read_local_abi(contract: &str) -> Option<Vec<u8>> {
    let cwd = std::env::current_dir().ok()?;
    read_local_abi_in(&cwd, contract)
}

fn read_local_abi_in(root: &std::path::Path, contract: &str) -> Option<Vec<u8>> {
    let abi_path = root.join("build").join(format!("{contract}.abi.json"));
    let abi = std::fs::read(abi_path).ok()?;
    labcoat_core::abi::validate(&abi).ok()?;
    Some(abi)
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

pub async fn wallet(ctx: &Ctx, cmd: WalletCmd, json: bool) -> (&'static str, CmdResult) {
    match cmd {
        WalletCmd::Init {
            mnemonic_stdin,
            show_mnemonic,
        } => {
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
                let mut result =
                    labcoat_core::wallet::init(&mut provider, &ctx.config, mnemonic, passphrase)
                        .await?;
                // Machine-readable output (CI logs, MCP transcripts) must not
                // capture the mnemonic unless explicitly asked to.
                if json && !show_mnemonic && result.mnemonic.is_some() {
                    result.mnemonic = None;
                    result.mnemonic_redacted = true;
                    ctx.warn("a mnemonic was generated but withheld from the JSON output; re-run with --show-mnemonic to capture it, or back up the keystore file");
                }
                Ok(result)
            }
            .await;
            ("wallet-init", to_envelope(res))
        }
        WalletCmd::Addresses { count } => {
            let res = async {
                let provider = ctx.wallet_provider().await?;
                labcoat_core::wallet::addresses(&provider, count).await
            }
            .await;
            ("wallet-addresses", to_envelope(res))
        }
        WalletCmd::Utxos => {
            let res = async {
                let provider = ctx.wallet_provider().await?;
                labcoat_core::wallet::utxos(&provider).await
            }
            .await;
            ("wallet-utxos", to_envelope(res))
        }
        WalletCmd::SignPsbt { input, output } => {
            let res = async {
                use labcoat_core::signer::Signer;
                let raw = std::fs::read_to_string(&input).map_err(|e| {
                    labcoat_core::LabcoatError::new(
                        "CONFIG_INVALID",
                        format!("cannot read {input}: {e}"),
                        "pass the unsigned PSBT file with --in",
                    )
                })?;
                let mut psbt = labcoat_core::signer::decode_psbt(raw.trim())?;
                let provider =
                    labcoat_core::system::connect(&ctx.config, ctx.passphrase(), true).await?;
                let signer = labcoat_core::signer::KeystoreSigner::from_provider(&provider)?;
                let signed = signer.sign_psbt(&mut psbt).await?;
                let output = output.unwrap_or_else(|| {
                    let path = std::path::Path::new(&input);
                    let stem = path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("request");
                    path.with_file_name(format!("{stem}.signed.psbt"))
                        .display()
                        .to_string()
                });
                std::fs::write(&output, labcoat_core::signer::encode_psbt(&psbt)).map_err(|e| {
                    labcoat_core::LabcoatError::new(
                        "TOOLKIT_ERROR",
                        format!("cannot write {output}: {e}"),
                        "check the output path permissions",
                    )
                })?;
                Ok(serde_json::json!({
                    "in": input,
                    "out": output,
                    "signedInputs": signed,
                    "walletFingerprint": signer
                        .fingerprint()
                        .map(|fingerprint| fingerprint.to_string()),
                }))
            }
            .await;
            ("wallet-sign-psbt", to_envelope(res))
        }
        WalletCmd::SignDigest { address, digest } => {
            let res = async {
                use labcoat_core::signer::Signer;
                let bytes = hex::decode(&digest).map_err(|_| {
                    labcoat_core::LabcoatError::new(
                        "CONFIG_INVALID",
                        "--digest must be exactly 32 bytes of hexadecimal",
                        "pass a 64-character digest without a 0x prefix",
                    )
                })?;
                let digest: [u8; 32] = bytes.try_into().map_err(|_| {
                    labcoat_core::LabcoatError::new(
                        "CONFIG_INVALID",
                        "--digest must be exactly 32 bytes",
                        "pass a 64-character hexadecimal digest",
                    )
                })?;
                let provider =
                    labcoat_core::system::connect(&ctx.config, ctx.passphrase(), true).await?;
                let signer = labcoat_core::signer::KeystoreSigner::from_provider(&provider)?;
                signer.sign_schnorr_digest(&address, digest).await
            }
            .await;
            ("wallet-sign-digest", to_envelope(res))
        }
    }
}

pub fn build(package: Option<&str>, out_dir: &str) -> (&'static str, CmdResult) {
    let res = build_selected(package, out_dir)
        .map(|selection| serde_json::json!({ "contracts": selection.outcomes }));
    ("build", to_envelope(res))
}

struct CompileSelection {
    workspace_root: PathBuf,
    outcomes: Vec<labcoat_core::compile::CompileOutcome>,
}

fn build_selected(
    package: Option<&str>,
    out_dir: &str,
) -> Result<CompileSelection, labcoat_core::LabcoatError> {
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
    Ok(CompileSelection {
        workspace_root: workspace.root,
        outcomes,
    })
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
                        "run `labcoat build <package>` first",
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
                        "build the deployed source revision or verify the target contract id",
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
    package: Option<&str>,
    wasm: Option<&str>,
    name: Option<String>,
    args: &[String],
    reserve: Option<u128>,
    tx: &TxFlags,
) -> (&'static str, CmdResult) {
    let res = async {
        let artifact = resolve_deployment_artifact(package, wasm, name)?;
        let constructor = resolve_constructor_args(ctx, &artifact.wasm_path, args)?;
        let outcome = toolkit::deploy_in(
            &ctx.config,
            &ctx.signer_spec()?,
            &artifact.deployment_root,
            toolkit::DeployRequest {
                wasm_path: &artifact.wasm_path,
                contract_name: Some(artifact.contract_name),
                cellpack_args: &constructor.cellpack,
                fee_rate: ctx.config.fee_rate,
                target: deploy_target(reserve),
                options: &tx.options(),
            },
        )
        .await?;
        let mut value = serde_json::to_value(outcome).expect("serializable deploy outcome");
        if let serde_json::Value::Object(object) = &mut value {
            object.insert(
                "argsEncoding".into(),
                serde_json::to_value(constructor.encoding).expect("serializable encoding"),
            );
            object.insert("constructor".into(), serde_json::json!(constructor.method));
        }
        Ok(value)
    }
    .await;
    ("deploy", to_envelope(res))
}

/// --dry-run deploy: validate the wasm payload and args, show the plan.
pub fn deploy_dry_run(
    ctx: &Ctx,
    package: Option<&str>,
    wasm: Option<&str>,
    name: Option<String>,
    args: &[String],
    reserve: Option<u128>,
    tx: &TxFlags,
) -> (&'static str, CmdResult) {
    let res = (|| {
        let artifact = resolve_deployment_artifact(package, wasm, name)?;
        let bytes = std::fs::read(&artifact.wasm_path).map_err(|e| {
            labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                format!("cannot read {}: {}", artifact.wasm_path.display(), e),
                "build the package or check the --wasm path",
            )
        })?;
        if bytes.starts_with(&[0x1f, 0x8b]) {
            return Err(labcoat_core::LabcoatError::new(
                "ENVELOPE_INVALID",
                "wasm payload is gzip-compressed; deploy wants the raw .wasm".to_string(),
                "pass the .wasm produced by `labcoat build`",
            ));
        }
        let constructor = resolve_constructor_args(ctx, &artifact.wasm_path, args)?;
        let options = tx.options();
        options.input_requirements()?;
        let target = deploy_target(reserve);
        let spec = toolkit::deploy_spec(target, &constructor.cellpack, &options);
        labcoat_core::execute::validate_spec(&spec)?;
        use sha2::Digest;
        Ok(serde_json::json!({
            "dryRun": true,
            "network": ctx.config.network_id(),
            "bitcoinNetwork": ctx.config.bitcoin_network_id(),
            "package": artifact.package,
            "wasm": artifact.wasm_path.display().to_string(),
            "wasmBytes": bytes.len(),
            "wasmSha256": hex::encode(sha2::Sha256::digest(&bytes)),
            "name": artifact.contract_name,
            "target": match target {
                toolkit::DeployTarget::New => "1:0 (next free id)".to_string(),
                toolkit::DeployTarget::Reserve(n) => format!("3:{n} (reserved)"),
            },
            "constructor": constructor.method,
            "argsEncoding": constructor.encoding,
            "cellpackArgs": constructor.cellpack.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
            "protostoneSpec": spec,
            "tx": tx.metadata(),
            "wouldBroadcast": "commit + reveal transactions with the wasm envelope",
        }))
    })();
    ("deploy", to_envelope(res))
}

struct DeploymentArtifact {
    package: Option<String>,
    wasm_path: PathBuf,
    contract_name: String,
    deployment_root: PathBuf,
}

fn resolve_deployment_artifact(
    package: Option<&str>,
    wasm: Option<&str>,
    name: Option<String>,
) -> Result<DeploymentArtifact, labcoat_core::LabcoatError> {
    match (package, wasm) {
        (Some(package), None) => {
            if name.is_some() {
                return Err(labcoat_core::LabcoatError::new(
                    "CONFIG_INVALID",
                    "--name is only valid with --wasm",
                    "omit --name when deploying a Cargo contract package",
                ));
            }
            let mut selection = build_selected(Some(package), "build")?;
            let outcome = selection.outcomes.pop().ok_or_else(|| {
                labcoat_core::LabcoatError::new(
                    "PACKAGE_NOT_FOUND",
                    format!("contract package `{package}` was not compiled"),
                    "pass an exact Cargo package name",
                )
            })?;
            Ok(package_deployment_artifact(
                selection.workspace_root,
                outcome,
            ))
        }
        (None, Some(wasm)) => raw_deployment_artifact(wasm, name),
        (Some(_), Some(_)) => Err(labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            "deploy accepts either a package or --wasm, not both",
            "use `labcoat deploy <package>` or `labcoat deploy --wasm <path>`",
        )),
        (None, None) => Err(labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            "deploy requires a contract package or --wasm path",
            "use `labcoat deploy <package>` or `labcoat deploy --wasm <path>`",
        )),
    }
}

fn package_deployment_artifact(
    workspace_root: PathBuf,
    outcome: labcoat_core::compile::CompileOutcome,
) -> DeploymentArtifact {
    DeploymentArtifact {
        package: Some(outcome.name.clone()),
        wasm_path: PathBuf::from(outcome.wasm_path),
        contract_name: outcome.name,
        deployment_root: workspace_root,
    }
}

fn raw_deployment_artifact(
    wasm: &str,
    name: Option<String>,
) -> Result<DeploymentArtifact, labcoat_core::LabcoatError> {
    let wasm_path = PathBuf::from(wasm);
    let contract_name = name
        .or_else(|| {
            wasm_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .ok_or_else(|| {
            labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                format!("cannot derive a contract name from {}", wasm_path.display()),
                "pass an explicit name with --name",
            )
        })?;
    let deployment_root = std::env::current_dir().map_err(|e| {
        labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            e.to_string(),
            "run Labcoat from a writable directory",
        )
    })?;
    Ok(DeploymentArtifact {
        package: None,
        wasm_path,
        contract_name,
        deployment_root,
    })
}

/// --dry-run call: resolve the contract, selector, and args, then show the plan.
pub async fn call_dry_run(
    ctx: &Ctx,
    contract: &str,
    selector: &str,
    args: &[String],
    tx_flags: &TxFlags,
) -> (&'static str, CmdResult) {
    let res = async {
        let (block, tx, invocation) =
            resolve_invocation(ctx, contract, selector, args, "call").await?;
        let options = tx_flags.options();
        options.input_requirements()?;
        let spec = toolkit::call_spec(block, tx, invocation.opcode, &invocation.cellpack_args, &options);
        labcoat_core::execute::validate_spec(&spec)?;
        Ok(serde_json::json!({
            "dryRun": true,
            "network": ctx.config.network_id(),
            "bitcoinNetwork": ctx.config.bitcoin_network_id(),
            "target": invocation.target,
            "targetRevision": "deployed",
            "selector": selector,
            "method": invocation.method,
            "opcode": invocation.opcode.to_string(),
            "abiSource": invocation.abi_source,
            "localBuildStatus": invocation.local_build_status,
            "cellpackArgs": invocation.cellpack_args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
            "protostoneSpec": spec,
            "tx": tx_flags.metadata(),
            "wouldBroadcast": "one execute transaction carrying the protostone",
        }))
    }
    .await;
    ("call", to_envelope(res))
}

pub async fn call(
    ctx: &Ctx,
    contract: &str,
    selector: &str,
    args: &[String],
    tx_flags: &TxFlags,
) -> (&'static str, CmdResult) {
    let res = async {
        let (block, tx, invocation) =
            resolve_invocation(ctx, contract, selector, args, "call").await?;
        let outcome = toolkit::call(
            &ctx.config,
            &ctx.signer_spec()?,
            toolkit::CallRequest {
                block,
                tx,
                opcode: invocation.opcode,
                args: &invocation.cellpack_args,
                fee_rate: ctx.config.fee_rate,
                options: &tx_flags.options(),
            },
        )
        .await?;
        Ok(invocation.enrich(serde_json::to_value(outcome).expect("serializable call outcome")))
    }
    .await;
    ("call", to_envelope(res))
}

pub async fn exchange(
    ctx: &Ctx,
    offered: &str,
    offered_amount: u64,
    payment: &str,
    payment_amount: u64,
    seller_wallet_file: &str,
) -> (&'static str, CmdResult) {
    let res = async {
        if !ctx.config.network.uses_regtest() {
            return Err(labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                "the two-keystore exchange coordinator is restricted to Labcoat Network and custom regtest",
                "use an external PSBT signer workflow for public networks",
            ));
        }
        let (offered_block, offered_tx) = resolve(&ctx.config, offered)?;
        let (payment_block, payment_tx) = resolve(&ctx.config, payment)?;
        let mut seller_config = ctx.config.clone();
        seller_config.wallet_file = PathBuf::from(seller_wallet_file);
        if seller_config.wallet_file == ctx.config.wallet_file {
            return Err(labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                "seller and buyer keystore paths must be different",
                "pass the seller keystore with --seller-wallet-file and the buyer with --wallet-file",
            ));
        }

        let passphrase = ctx.passphrase();
        ctx.config.require_passphrase_policy(&passphrase)?;
        let mut buyer = labcoat_core::system::connect(&ctx.config, passphrase.clone(), true).await?;
        let seller = labcoat_core::system::connect(&seller_config, passphrase, true).await?;
        let buyer_address = labcoat_core::atomic_exchange::primary_address(&buyer).await?;
        let seller_address = labcoat_core::atomic_exchange::primary_address(&seller).await?;

        let outcome = labcoat_core::atomic_exchange::run(
            &mut buyer,
            &seller,
            &ctx.config,
            labcoat_core::atomic_exchange::AtomicExchangeRequest {
                offered: labcoat_core::atomic_exchange::AlkaneId {
                    block: u64::try_from(offered_block).map_err(|_| {
                        labcoat_core::LabcoatError::new(
                            "CONFIG_INVALID",
                            format!("offered asset block {offered_block} does not fit in u64"),
                            "use a valid on-chain Alkane ID",
                        )
                    })?,
                    tx: u64::try_from(offered_tx).map_err(|_| {
                        labcoat_core::LabcoatError::new(
                            "CONFIG_INVALID",
                            format!("offered asset tx {offered_tx} does not fit in u64"),
                            "use a valid on-chain Alkane ID",
                        )
                    })?,
                },
                offered_amount,
                payment: labcoat_core::atomic_exchange::AlkaneId {
                    block: u64::try_from(payment_block).map_err(|_| {
                        labcoat_core::LabcoatError::new(
                            "CONFIG_INVALID",
                            format!("payment asset block {payment_block} does not fit in u64"),
                            "use a valid on-chain Alkane ID",
                        )
                    })?,
                    tx: u64::try_from(payment_tx).map_err(|_| {
                        labcoat_core::LabcoatError::new(
                            "CONFIG_INVALID",
                            format!("payment asset tx {payment_tx} does not fit in u64"),
                            "use a valid on-chain Alkane ID",
                        )
                    })?,
                },
                payment_amount,
                seller_address,
                buyer_address,
            },
        )
        .await?;
        Ok(serde_json::to_value(outcome).expect("serializable exchange outcome"))
    }
    .await;
    ("exchange", to_envelope(res))
}

fn exchange_asset(
    id: (u128, u128),
    label: &str,
) -> Result<labcoat_core::atomic_exchange::AlkaneId, labcoat_core::LabcoatError> {
    Ok(labcoat_core::atomic_exchange::AlkaneId {
        block: u64::try_from(id.0).map_err(|_| {
            labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                format!("{label} block {} does not fit u64", id.0),
                "use a valid on-chain Alkane ID",
            )
        })?,
        tx: u64::try_from(id.1).map_err(|_| {
            labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                format!("{label} tx {} does not fit u64", id.1),
                "use a valid on-chain Alkane ID",
            )
        })?,
    })
}

#[allow(clippy::too_many_arguments)]
/// Where an `exchange-plan` request comes from: the positional/flag form,
/// or a version-1 exchange request file (`--request`).
pub enum ExchangeRequestSource {
    Flags {
        offered: String,
        offered_amount: u64,
        payment: String,
        payment_amount: u64,
        seller_address: String,
        buyer_address: String,
    },
    File {
        path: String,
    },
}

pub async fn exchange_plan(
    ctx: &Ctx,
    source: ExchangeRequestSource,
    plan_out: &str,
    psbt_out: &str,
) -> (&'static str, CmdResult) {
    let res = async {
        let request = match source {
            ExchangeRequestSource::Flags {
                offered,
                offered_amount,
                payment,
                payment_amount,
                seller_address,
                buyer_address,
            } => labcoat_core::atomic_exchange::AtomicExchangeRequest {
                offered: exchange_asset(resolve(&ctx.config, &offered)?, "offered asset")?,
                offered_amount,
                payment: exchange_asset(resolve(&ctx.config, &payment)?, "payment asset")?,
                payment_amount,
                seller_address,
                buyer_address,
            },
            ExchangeRequestSource::File { path } => {
                let json = std::fs::read_to_string(&path).map_err(|e| {
                    labcoat_core::LabcoatError::new(
                        "CONFIG_INVALID",
                        format!("cannot read {path}: {e}"),
                        "pass an exchange request file produced by your application",
                    )
                })?;
                labcoat_core::atomic_exchange::AtomicExchangeRequest::from_request_file(&json)?
            }
        };
        let mut provider = ctx.wallet_provider().await?;
        let plan = labcoat_core::atomic_exchange::build_exchange_plan(
            &mut provider,
            &ctx.config,
            request,
        )
        .await?;
        let plan_json = serde_json::to_string_pretty(&plan).expect("serializable exchange plan");
        std::fs::write(plan_out, format!("{plan_json}\n")).map_err(|e| {
            labcoat_core::LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot write {plan_out}: {e}"),
                "check the plan output path",
            )
        })?;
        std::fs::write(psbt_out, format!("{}\n", plan.psbt)).map_err(|e| {
            labcoat_core::LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot write {psbt_out}: {e}"),
                "check the PSBT output path",
            )
        })?;
        Ok(serde_json::json!({ "plan": plan, "planOut": plan_out, "psbtOut": psbt_out }))
    }
    .await;
    ("exchange-plan", to_envelope(res))
}

pub async fn exchange_settle(
    ctx: &Ctx,
    plan_path: &str,
    psbt_path: &str,
    seller_wallet_file: &str,
    broadcast: bool,
) -> (&'static str, CmdResult) {
    let res = async {
        let plan_raw = std::fs::read_to_string(plan_path).map_err(|e| {
            labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                format!("cannot read {plan_path}: {e}"),
                "pass the ExchangePlanV1 JSON file",
            )
        })?;
        let plan: labcoat_core::atomic_exchange::ExchangePlanV1 = serde_json::from_str(&plan_raw)
            .map_err(|e| {
            labcoat_core::LabcoatError::new(
                "EXCHANGE_PLAN_INVALID",
                format!("cannot decode exchange plan: {e}"),
                "recreate the plan with `labcoat exchange-plan`",
            )
        })?;
        let psbt_raw = std::fs::read_to_string(psbt_path).map_err(|e| {
            labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                format!("cannot read {psbt_path}: {e}"),
                "pass the buyer-signed PSBT file",
            )
        })?;
        let psbt = labcoat_core::signer::decode_psbt(psbt_raw.trim())?;
        let mut seller_config = ctx.config.clone();
        seller_config.wallet_file = PathBuf::from(seller_wallet_file);
        let connected =
            labcoat_core::system::connect_signing(&seller_config, &ctx.signer_spec()?).await?;
        let mut provider = connected.provider;
        let outcome = labcoat_core::atomic_exchange::settle_exchange(
            &mut provider,
            connected.signer.as_ref(),
            &seller_config,
            &plan,
            psbt,
            broadcast,
        )
        .await?;
        Ok(outcome)
    }
    .await;
    ("exchange-settle", to_envelope(res))
}

pub async fn simulate(
    ctx: &Ctx,
    contract: &str,
    selector: &str,
    args: &[String],
) -> (&'static str, CmdResult) {
    let res =
        async {
            let (block, tx, invocation) =
                resolve_invocation(ctx, contract, selector, args, "simulate").await?;
            let outcome = toolkit::simulate(
                &ctx.config,
                block,
                tx,
                invocation.opcode,
                &invocation.cellpack_args,
            )
            .await?;
            Ok(invocation
                .enrich(serde_json::to_value(outcome).expect("serializable simulate outcome")))
        }
        .await;
    ("simulate", to_envelope(res))
}

fn manifest_root_and_path(
    manifest: Option<&str>,
) -> Result<(PathBuf, PathBuf), labcoat_core::LabcoatError> {
    let root = std::env::current_dir().map_err(|e| {
        labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            e.to_string(),
            "run Labcoat from a readable directory",
        )
    })?;
    let path = match manifest {
        Some(path) => root.join(path),
        None => root.join(labcoat_core::manifest::MANIFEST),
    };
    Ok((root, path))
}

/// `labcoat plan` — reconcile the manifest, never load a signer.
pub async fn plan(ctx: &Ctx, manifest: Option<&str>) -> (&'static str, CmdResult) {
    let res = async {
        let (root, path) = manifest_root_and_path(manifest)?;
        let outcome = labcoat_core::apply::plan(&ctx.config, &root, &path).await?;
        Ok(serde_json::to_value(outcome.plan).expect("serializable plan"))
    }
    .await;
    ("plan", to_envelope(res))
}

/// `labcoat apply` — execute the plan's pending actions under --broadcast;
/// without the flag it only shows the plan.
pub async fn apply(
    ctx: &Ctx,
    manifest: Option<&str>,
    broadcast: bool,
) -> (&'static str, CmdResult) {
    let res = async {
        let (root, path) = manifest_root_and_path(manifest)?;
        if !broadcast {
            let outcome = labcoat_core::apply::plan(&ctx.config, &root, &path).await?;
            let mut value = serde_json::to_value(outcome.plan).expect("serializable plan");
            if let serde_json::Value::Object(object) = &mut value {
                object.insert("dryRun".into(), serde_json::json!(true));
                object.insert(
                    "wouldBroadcast".into(),
                    serde_json::json!("re-run with --broadcast to execute the pending actions"),
                );
            }
            return Ok(value);
        }
        let report =
            labcoat_core::apply::apply(&ctx.config, &ctx.signer_spec()?, &root, &path).await?;
        Ok(serde_json::to_value(report).expect("serializable apply report"))
    }
    .await;
    ("apply", to_envelope(res))
}

pub async fn trace(ctx: &Ctx, txid: &str, wait: bool) -> (&'static str, CmdResult) {
    let res = toolkit::trace(&ctx.config, txid, wait)
        .await
        .map(|traces| serde_json::json!({ "txid": txid, "traces": traces }));
    ("trace", to_envelope(res))
}

pub async fn balance(ctx: &Ctx, address: &str) -> (&'static str, CmdResult) {
    let res = toolkit::balances(&ctx.config, address)
        .await
        .map(|balances| serde_json::json!({ "address": address, "balances": balances }));
    ("balance", to_envelope(res))
}

pub fn generate(ctx: &Ctx, cmd: GenerateCmd) -> (&'static str, CmdResult) {
    match cmd {
        GenerateCmd::Web { out_dir, build_dir } => {
            let res: Result<serde_json::Value, labcoat_core::LabcoatError> = (|| {
                let cwd = std::env::current_dir().map_err(|e| {
                    labcoat_core::LabcoatError::new(
                        "CONFIG_INVALID",
                        e.to_string(),
                        "run Labcoat from the project root",
                    )
                })?;
                let lockfile = labcoat_core::lockfile::load(&cwd)?;
                let abis = labcoat_core::generate::load_build_abis(&cwd.join(&build_dir))?;
                let inputs = labcoat_core::generate::WebInputs {
                    network: ctx.config.network_id(),
                    bitcoin_network: ctx.config.bitcoin_network_id(),
                    rpc_url: &ctx.config.rpc_url,
                    lockfile: &lockfile,
                    abis: &abis,
                };
                let files = labcoat_core::generate::generate_web(&inputs)?;
                let out_root = cwd.join(&out_dir);
                labcoat_core::generate::write_files(&out_root, &files)?;
                let contracts = lockfile
                    .networks
                    .get(ctx.config.network_id())
                    .map(|network| network.len())
                    .unwrap_or(0);
                Ok(serde_json::json!({
                    "network": ctx.config.network_id(),
                    "outDir": out_dir,
                    "contracts": contracts,
                    "abis": abis.iter().map(|a| a.artifact.as_str()).collect::<Vec<_>>(),
                    "files": files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
                }))
            })();
            ("generate-web", to_envelope(res))
        }
    }
}

pub fn lock(cmd: LockCmd) -> (&'static str, CmdResult) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    match cmd {
        LockCmd::Show => (
            "lock-show",
            to_envelope(
                labcoat_core::lockfile::load(&cwd)
                    .map(|lockfile| serde_json::to_value(lockfile).expect("serializable")),
            ),
        ),
    }
}

#[cfg(test)]
mod deployment_tests {
    use super::*;

    fn abi_test_root(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "labcoat-abi-resolution-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn raw_wasm_uses_file_stem_unless_name_is_explicit() {
        let derived = raw_deployment_artifact("/tmp/counter.wasm", None).unwrap();
        assert_eq!(derived.package, None);
        assert_eq!(derived.contract_name, "counter");
        assert_eq!(derived.wasm_path, PathBuf::from("/tmp/counter.wasm"));

        let named = raw_deployment_artifact("/tmp/counter.wasm", Some("custom".into())).unwrap();
        assert_eq!(named.contract_name, "custom");
    }

    #[test]
    fn package_artifact_keeps_canonical_name_and_workspace_root() {
        let root = PathBuf::from("/workspace");
        let artifact = package_deployment_artifact(
            root.clone(),
            labcoat_core::compile::CompileOutcome {
                name: "my-token".into(),
                wasm_path: "/workspace/build/my-token.wasm".into(),
                wasm_gz_path: "/workspace/build/my-token.wasm.gz".into(),
                abi_path: "/workspace/build/my-token.abi.json".into(),
                wasm_sha256: "hash".into(),
            },
        );
        assert_eq!(artifact.package.as_deref(), Some("my-token"));
        assert_eq!(artifact.contract_name, "my-token");
        assert_eq!(artifact.deployment_root, root);
    }

    #[test]
    fn deployment_source_validation_rejects_missing_or_conflicting_inputs() {
        assert!(resolve_deployment_artifact(None, None, None).is_err());
        assert!(resolve_deployment_artifact(Some("counter"), Some("counter.wasm"), None).is_err());
    }

    #[test]
    fn distinguishes_numeric_opcodes_from_abi_method_names() {
        assert_eq!(numeric_selector("0").unwrap(), Some(0));
        assert_eq!(
            numeric_selector(&u128::MAX.to_string()).unwrap(),
            Some(u128::MAX)
        );
        assert_eq!(numeric_selector("increment").unwrap(), None);
        assert_eq!(numeric_selector("increment()").unwrap(), None);

        let overflow = format!("{}0", u128::MAX);
        let error = numeric_selector(&overflow).unwrap_err();
        assert_eq!(error.code, "CONFIG_INVALID");
        assert!(error.message.contains("does not fit in u128"));
        assert!(numeric_selector("").is_err());
    }

    #[test]
    fn matching_deployment_hash_uses_valid_generated_abi() {
        let root = abi_test_root("matching");
        let build = root.join("build");
        std::fs::create_dir_all(&build).unwrap();
        let wasm = b"matching wasm";
        let abi = br#"{"contract":"Registry","methods":[{"name":"store","opcode":1,"params":[{"name":"name","type":"String"}],"returns":"void"}]}"#;
        std::fs::write(build.join("name-registry.wasm"), wasm).unwrap();
        std::fs::write(build.join("name-registry.abi.json"), abi).unwrap();
        use sha2::Digest;
        let hash = hex::encode(sha2::Sha256::digest(wasm));

        assert_eq!(
            local_build_status_in(&root, "name-registry", &hash),
            LocalBuildStatus::Matches
        );
        assert_eq!(
            read_local_abi_in(&root, "name-registry").as_deref(),
            Some(abi.as_slice())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changed_or_missing_local_wasm_does_not_match_the_deployment() {
        let root = abi_test_root("drift");
        let build = root.join("build");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("name-registry.wasm"), b"new wasm").unwrap();

        use sha2::Digest;
        let deployed_hash = hex::encode(sha2::Sha256::digest(b"old wasm"));
        assert_eq!(
            local_build_status_in(&root, "name-registry", &deployed_hash),
            LocalBuildStatus::Differs
        );
        assert_eq!(
            local_build_status_in(&root, "missing", &deployed_hash),
            LocalBuildStatus::Unavailable
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_or_missing_local_abi_is_unavailable() {
        let root = abi_test_root("invalid-abi");
        let build = root.join("build");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("name-registry.abi.json"), b"not json").unwrap();

        assert_eq!(read_local_abi_in(&root, "name-registry"), None);
        assert_eq!(read_local_abi_in(&root, "missing"), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invocation_results_expose_deployed_target_and_abi_source() {
        let invocation = ResolvedInvocation {
            opcode: 7,
            method: Some("store".into()),
            cellpack_args: vec![1],
            target: "4:2".into(),
            abi_source: Some(AbiSource::LocalBuild),
            local_build_status: LocalBuildStatus::Matches,
        };
        let result = invocation.enrich(serde_json::json!({ "status": "success" }));
        assert_eq!(result["target"], "4:2");
        assert_eq!(result["targetRevision"], "deployed");
        assert_eq!(result["abiSource"], "local-build");
        assert_eq!(result["localBuildStatus"], "matches");
        assert_eq!(result["method"], "store");
        assert_eq!(result["opcode"], "7");

        let numeric = ResolvedInvocation {
            opcode: 8,
            method: None,
            cellpack_args: vec![],
            target: "4:2".into(),
            abi_source: None,
            local_build_status: LocalBuildStatus::Unavailable,
        };
        assert!(numeric.metadata()["abiSource"].is_null());
        assert!(numeric.metadata()["method"].is_null());
    }
}
