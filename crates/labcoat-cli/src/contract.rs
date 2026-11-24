//! Contract-operation subcommands: thin argument handling over
//! labcoat_core::toolkit, sharing the CLI's JSON envelope conventions.

use clap::Subcommand;
use labcoat_core::{toolkit, NetworkTarget, ToolkitConfig};
use std::io::Read;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum WalletCmd {
    /// Create (or load) the selected network's wallet; Labcoat Network
    /// already has one after `labcoat up`. Mnemonic is read from
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
pub enum StateCmd {
    /// List resources and active instances in this environment's durable
    /// state
    List,
    /// Show one resource's active instance (with --history, every instance)
    Show {
        /// Resource address ("contract.counter") or bare contract name
        resource: String,
        /// Include the full append-only instance history
        #[arg(long)]
        history: bool,
    },
    /// Create version-2 durable state from the v1 labcoat.lock ledger,
    /// backing the ledger up first. labcoat.lock stays in place as the
    /// active-address book.
    Migrate,
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
                environment: "default".to_string(),
                labcoat_instance_id: None,
            },
            color: crate::output::ColorMode::Auto,
            signer: "keystore".to_string(),
        }
    }

    pub fn with_color(mut self, color: crate::output::ColorMode) -> Self {
        self.color = color;
        self
    }

    pub fn with_environment(mut self, environment: &str) -> Self {
        self.config.environment = environment.to_string();
        self
    }

    /// Config for state-changing commands: on the managed Labcoat Network
    /// the persistent instance UUID is attached (read-or-create) so
    /// durable state can detect a reset chain before broadcasting.
    fn mutating_config(&self) -> ToolkitConfig {
        let mut config = self.config.clone();
        if config.network == NetworkTarget::Labcoat {
            config.labcoat_instance_id = isomer_core::instance_id().ok();
        }
        config
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

    /// The built-in wallet's passphrase is public, so it never holds a
    /// mnemonic that could also control real coins.
    pub fn refuse_imported_mnemonic(
        &self,
        mnemonic: &Option<String>,
    ) -> Result<(), labcoat_core::LabcoatError> {
        if mnemonic.is_some() && crate::local_wallet::is_built_in(&self.config.wallet_file) {
            return Err(labcoat_core::LabcoatError::new(
                "CONFIG_INVALID",
                "a mnemonic cannot be imported into the built-in Labcoat Network wallet, whose passphrase is public",
                "pass --wallet-file <path> to keep it in a wallet of its own",
            ));
        }
        Ok(())
    }

    /// Nobody should write down, and later reuse, the built-in wallet's
    /// mnemonic.
    pub fn hide_built_in_mnemonic(&self, result: &mut labcoat_core::wallet::WalletInitResult) {
        if crate::local_wallet::is_built_in(&self.config.wallet_file) {
            result.mnemonic = None;
        }
    }

    /// Wallet passphrase: the built-in wallet's is fixed; otherwise env var first; on regtest a loud dev default; on
    /// public networks an interactive prompt when a terminal is available
    /// (CI keeps using the env var).
    pub fn passphrase(&self) -> Option<String> {
        if crate::local_wallet::is_built_in(&self.config.wallet_file) {
            return Some(crate::local_wallet::PASSPHRASE.to_string());
        }
        match std::env::var("LABCOAT_WALLET_PASSPHRASE") {
            Ok(p) if !p.is_empty() => Some(p),
            _ => {
                if self.config.network.uses_regtest() {
                    self.warn("LABCOAT_WALLET_PASSPHRASE not set — using the fixed development passphrase (Labcoat Network and custom regtest only)");
                    Some(crate::local_wallet::PASSPHRASE.to_string())
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

    /// True when no transaction-shaping flag was given.
    fn is_empty(&self) -> bool {
        self.inputs.is_none()
            && self.to.is_none()
            && self.pointer.is_none()
            && self.refund.is_none()
            && self.edicts.is_empty()
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
    abi_contract: Option<&str>,
    operation: &str,
) -> Result<(u128, u128, ResolvedInvocation), labcoat_core::LabcoatError> {
    let numeric = numeric_selector(selector)?;
    let (block, tx) = resolve(&ctx.config, contract)?;
    let target = format!("{block}:{tx}");
    let deployment = named_deployment(&ctx.config, contract)?;
    let mut local_build_status = deployment
        .as_ref()
        .map(|deployment| local_build_status(&ctx.config, contract, deployment))
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
        let local_abi = if abi_contract.is_none() && local_build_status == LocalBuildStatus::Matches
        {
            read_local_abi(&ctx.config, contract)
        } else {
            None
        };
        let (abi, abi_source) = if let Some(abi_contract) = abi_contract {
            // A proxy's own ABI lists only proxy methods: resolve the
            // method against the named implementation's deployed ABI, and
            // still send the call to `contract`.
            let (abi_block, abi_tx) = resolve(&ctx.config, abi_contract)?;
            (
                labcoat_core::abi::fetch_deployed(&ctx.config, abi_block, abi_tx).await?,
                AbiSource::DeployedMeta,
            )
        } else if let Some(abi) = local_abi {
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

/// The build artifact (file stem under `build/`) a lock name was deployed
/// from. A `deploy.rs` step records it, because a step's name need not be
/// its package's — a proxy step named `token` is not the `token` package.
/// A step with no local artifact (a template clone) has none; any other
/// name is assumed to be a package deployed under its own name.
fn local_artifact_stem(
    root: &std::path::Path,
    config: &ToolkitConfig,
    contract: &str,
) -> Option<String> {
    let record = labcoat_core::runfile::load(root, config.network_id())
        .ok()
        .and_then(|run| run.steps.get(contract).cloned())
        .filter(|record| record.kind == "contract");
    match record {
        Some(record) => record.artifact,
        None => Some(contract.to_string()),
    }
}

fn local_build_status(
    config: &ToolkitConfig,
    contract: &str,
    deployment: &labcoat_core::lockfile::Deployment,
) -> LocalBuildStatus {
    let Some(expected_hash) = deployment.wasm_sha256.as_deref() else {
        return LocalBuildStatus::Unavailable;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    match local_artifact_stem(&cwd, config, contract) {
        Some(stem) => local_build_status_in(&cwd, &stem, expected_hash),
        None => LocalBuildStatus::Unavailable,
    }
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

fn read_local_abi(config: &ToolkitConfig, contract: &str) -> Option<Vec<u8>> {
    let cwd = std::env::current_dir().ok()?;
    read_local_abi_in(&cwd, &local_artifact_stem(&cwd, config, contract)?)
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
                ctx.refuse_imported_mnemonic(&mnemonic)?;
                let mut provider =
                    labcoat_core::system::connect(&ctx.config, passphrase.clone(), false).await?;
                let mut result =
                    labcoat_core::wallet::init(&mut provider, &ctx.config, mnemonic, passphrase)
                        .await?;
                ctx.hide_built_in_mnemonic(&mut result);
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
        .map(|outcomes| serde_json::json!({ "contracts": outcomes }));
    ("build", to_envelope(res))
}

fn build_selected(
    package: Option<&str>,
    out_dir: &str,
) -> Result<Vec<labcoat_core::compile::CompileOutcome>, labcoat_core::LabcoatError> {
    let cwd = std::env::current_dir().map_err(|e| {
        labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            e.to_string(),
            "run Labcoat from a Cargo workspace",
        )
    })?;
    let workspace = labcoat_core::workspace::discover(&cwd)?;
    let packages = labcoat_core::workspace::select(&workspace, package)?;
    labcoat_core::compile::compile_packages(
        &workspace,
        &packages,
        &PathBuf::from(out_dir),
        "wasm32-unknown-unknown",
    )
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

/// What `labcoat deploy` (or the `deploy` MCP tool) was asked to deploy.
#[derive(Debug)]
pub enum DeployRequest {
    /// Steps deploy.rs declares: every pending one, or the named one.
    Steps {
        only: Option<String>,
        replace: Vec<String>,
    },
    /// A prebuilt artifact, deployed outside deploy.rs.
    Wasm {
        path: String,
        name: Option<String>,
        args: Vec<String>,
        reserve: Option<u128>,
        tx: TxFlags,
    },
}

/// The deploy flags as given, before they are checked against each other.
#[derive(Default)]
pub struct DeployFlags {
    pub step: Option<String>,
    pub replace: Vec<String>,
    pub wasm: Option<String>,
    pub name: Option<String>,
    pub args: Vec<String>,
    pub reserve: Option<u128>,
    pub tx: TxFlags,
}

impl DeployFlags {
    /// Sort the flags into one of the two things deploy does. A flag that
    /// belongs to the other one is an error rather than silently ignored.
    pub fn request(self) -> Result<DeployRequest, EnvelopeError> {
        let invalid = |message: &str, hint: &'static str| EnvelopeError {
            code: "CONFIG_INVALID",
            message: message.to_string(),
            hint,
        };
        match self.wasm {
            Some(path) => {
                if self.step.is_some() || !self.replace.is_empty() {
                    return Err(invalid(
                        "--wasm deploys a prebuilt artifact outside deploy.rs, so it takes no step name or --replace",
                        "drop --wasm to deploy a deploy.rs step, or drop the step name and --replace",
                    ));
                }
                Ok(DeployRequest::Wasm {
                    path,
                    name: self.name,
                    args: self.args,
                    reserve: self.reserve,
                    tx: self.tx,
                })
            }
            None => {
                if self.name.is_some()
                    || !self.args.is_empty()
                    || self.reserve.is_some()
                    || !self.tx.is_empty()
                {
                    return Err(invalid(
                        "--name, --args, --reserve, and the transaction flags apply only to --wasm deploys",
                        "declare a step's arguments, reserve number, and transaction shape in deploy.rs (`.arg(name, value)`, `.reserve(n)`, `.inputs(..)`)",
                    ));
                }
                Ok(DeployRequest::Steps {
                    only: self.step,
                    replace: self.replace,
                })
            }
        }
    }
}

/// `labcoat deploy` — execute deploy.rs's pending steps (or the named one)
/// or deploy the given artifact; with `dry_run`, show what would happen
/// without loading a signer. Signing itself asks for approval on public
/// networks.
pub async fn deploy(ctx: &Ctx, request: DeployRequest, dry_run: bool) -> (&'static str, CmdResult) {
    let res = match request {
        DeployRequest::Steps { only, replace } => deploy_steps(ctx, only, &replace, dry_run).await,
        DeployRequest::Wasm {
            path,
            name,
            args,
            reserve,
            tx,
        } => {
            if dry_run {
                deploy_wasm_dry_run(ctx, &path, name, &args, reserve, &tx)
            } else {
                deploy_wasm(ctx, &path, name, &args, reserve, &tx).await
            }
        }
    };
    ("deploy", to_envelope(res))
}

/// Deploy.rs steps: the plan under `dry_run`, otherwise the executed report.
async fn deploy_steps(
    ctx: &Ctx,
    only: Option<String>,
    replace: &[String],
    dry_run: bool,
) -> Result<serde_json::Value, labcoat_core::LabcoatError> {
    let root = project_root()?;
    let graph = crate::deploy_script::load_graph(&root, ctx.config.network_id())?;
    let options = deployment_options(only, replace);
    if dry_run {
        let outcome = labcoat_core::apply::plan(&ctx.config, &root, &graph, &options).await?;
        let mut value = serde_json::to_value(outcome.plan).expect("serializable plan");
        if let serde_json::Value::Object(object) = &mut value {
            object.insert("dryRun".into(), serde_json::json!(true));
            object.insert(
                "wouldBroadcast".into(),
                serde_json::json!("re-run without --dry-run to execute the pending steps"),
            );
        }
        return Ok(value);
    }
    let report = labcoat_core::apply::apply(
        &ctx.mutating_config(),
        &ctx.signer_spec()?,
        &root,
        &graph,
        &options,
    )
    .await?;
    Ok(serde_json::to_value(report).expect("serializable deploy report"))
}

/// A prebuilt artifact outside deploy.rs, recorded in labcoat.lock only.
async fn deploy_wasm(
    ctx: &Ctx,
    wasm: &str,
    name: Option<String>,
    args: &[String],
    reserve: Option<u128>,
    tx: &TxFlags,
) -> Result<serde_json::Value, labcoat_core::LabcoatError> {
    let artifact = raw_deployment_artifact(wasm, name)?;
    // labcoat.lock maps one name to one contract. Taking a step's name
    // here would leave the deployment's record pointing at another one.
    if let Some(kind) = labcoat_core::apply::recorded_step_kind(
        &ctx.config,
        &artifact.deployment_root,
        &artifact.contract_name,
    )
    .await?
    {
        return Err(labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            format!(
                "`{}` is already the name of a deploy.rs {kind} step on {}",
                artifact.contract_name,
                ctx.config.network_id()
            ),
            "pass --name <other> to deploy this artifact outside deploy.rs, or drop --wasm to deploy the step",
        ));
    }
    let constructor = resolve_constructor_args(ctx, &artifact.wasm_path, args)?;
    let outcome = toolkit::deploy_in(
        &ctx.mutating_config(),
        &ctx.signer_spec()?,
        &artifact.deployment_root,
        toolkit::DeployRequest {
            wasm_path: &artifact.wasm_path,
            contract_name: Some(artifact.contract_name),
            initializer: None,
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

/// --dry-run for a --wasm deploy: validate the payload and args offline
/// and show what would be sent.
fn deploy_wasm_dry_run(
    ctx: &Ctx,
    wasm: &str,
    name: Option<String>,
    args: &[String],
    reserve: Option<u128>,
    tx: &TxFlags,
) -> Result<serde_json::Value, labcoat_core::LabcoatError> {
    let artifact = raw_deployment_artifact(wasm, name)?;
    let bytes = std::fs::read(&artifact.wasm_path).map_err(|e| {
        labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            format!("cannot read {}: {}", artifact.wasm_path.display(), e),
            "check the --wasm path, or build it with `labcoat build <package>`",
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
        "wasm": artifact.wasm_path.display().to_string(),
        "wasmBytes": bytes.len(),
        "wasmSha256": hex::encode(sha2::Sha256::digest(&bytes)),
        "name": artifact.contract_name,
        "target": match target {
            toolkit::DeployTarget::New => "1:0 (next free id)".to_string(),
            toolkit::DeployTarget::Reserve(n) => format!("3:{n} (reserved)"),
            clone @ toolkit::DeployTarget::Clone { .. } => {
                format!("{} (template clone)", clone.label())
            }
        },
        "constructor": constructor.method,
        "argsEncoding": constructor.encoding,
        "cellpackArgs": constructor.cellpack.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        "protostoneSpec": spec,
        "tx": tx.metadata(),
        "wouldBroadcast": "commit + reveal transactions with the wasm envelope",
    }))
}

struct DeploymentArtifact {
    wasm_path: PathBuf,
    contract_name: String,
    deployment_root: PathBuf,
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
    abi_contract: Option<&str>,
) -> (&'static str, CmdResult) {
    let res = async {
        let (block, tx, invocation) =
            resolve_invocation(ctx, contract, selector, args, abi_contract, "call").await?;
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
    abi_contract: Option<&str>,
) -> (&'static str, CmdResult) {
    let res = async {
        let (block, tx, invocation) =
            resolve_invocation(ctx, contract, selector, args, abi_contract, "call").await?;
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
        let plan =
            labcoat_core::atomic_exchange::build_exchange_plan(&mut provider, &ctx.config, request)
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
    abi_contract: Option<&str>,
) -> (&'static str, CmdResult) {
    let res =
        async {
            let (block, tx, invocation) =
                resolve_invocation(ctx, contract, selector, args, abi_contract, "simulate").await?;
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

fn project_root() -> Result<PathBuf, labcoat_core::LabcoatError> {
    std::env::current_dir().map_err(|e| {
        labcoat_core::LabcoatError::new(
            "CONFIG_INVALID",
            e.to_string(),
            "run Labcoat from a readable directory",
        )
    })
}

fn deployment_options(only: Option<String>, replace: &[String]) -> labcoat_core::apply::Options {
    labcoat_core::apply::Options {
        replace: replace.iter().cloned().collect(),
        only,
        ..Default::default()
    }
}

/// `labcoat plan` — run deploy.rs and reconcile its graph; never loads a
/// signer.
pub async fn plan(ctx: &Ctx, replace: &[String]) -> (&'static str, CmdResult) {
    let res = async {
        let root = project_root()?;
        let graph = crate::deploy_script::load_graph(&root, ctx.config.network_id())?;
        let outcome = labcoat_core::apply::plan(
            &ctx.config,
            &root,
            &graph,
            &deployment_options(None, replace),
        )
        .await?;
        Ok(serde_json::to_value(outcome.plan).expect("serializable plan"))
    }
    .await;
    ("plan", to_envelope(res))
}

/// `labcoat import <name> <id>` — record an existing on-chain contract as
/// a deployment step's instance. Broadcasts nothing.
pub async fn import(
    ctx: &Ctx,
    name: &str,
    id: &str,
    txid: Option<&str>,
) -> (&'static str, CmdResult) {
    let res = async {
        let root = project_root()?;
        let graph = crate::deploy_script::load_graph(&root, ctx.config.network_id())?;
        let report =
            labcoat_core::apply::import(&ctx.config, &root, &graph, name, id, txid).await?;
        Ok(serde_json::to_value(report).expect("serializable import report"))
    }
    .await;
    ("import", to_envelope(res))
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

fn load_state_or_missing(
    root: &std::path::Path,
    environment: &str,
) -> Result<labcoat_core::state::State, labcoat_core::LabcoatError> {
    labcoat_core::state_backend::load(root, environment)?.ok_or_else(|| {
        labcoat_core::LabcoatError::new(
            "STATE_MISSING",
            format!("no durable state for environment '{environment}'"),
            "run `labcoat state migrate` to create durable state from labcoat.lock",
        )
    })
}

pub async fn state(ctx: &Ctx, environment: &str, cmd: StateCmd) -> (&'static str, CmdResult) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    match cmd {
        StateCmd::List => {
            let res = (|| {
                let state = load_state_or_missing(&cwd, environment)?;
                let resources: Vec<serde_json::Value> = state
                    .resources
                    .iter()
                    .map(|(address, resource)| {
                        let active = resource
                            .active_instance
                            .as_deref()
                            .and_then(|id| resource.instances.iter().find(|i| i.instance_id == id));
                        serde_json::json!({
                            "address": address,
                            "kind": resource.kind,
                            "activeInstance": resource.active_instance,
                            "alkanesId": active.map(|i| i.alkanes_id.clone()),
                            "status": active.map(|i| i.status.clone()),
                            "instances": resource.instances.len(),
                        })
                    })
                    .collect();
                Ok(serde_json::json!({
                    "environment": state.environment,
                    "lineage": state.lineage,
                    "serial": state.serial,
                    "chain": state.chain,
                    "resources": resources,
                }))
            })();
            ("state-list", to_envelope(res))
        }
        StateCmd::Show { resource, history } => {
            let res = (|| {
                let state = load_state_or_missing(&cwd, environment)?;
                let address = if state.resources.contains_key(&resource) {
                    resource.clone()
                } else {
                    labcoat_core::state::resource_address(&resource)
                };
                let record = state.resources.get(&address).ok_or_else(|| {
                    labcoat_core::LabcoatError::new(
                        "CONTRACT_NOT_FOUND",
                        format!(
                            "no resource '{address}' in environment '{}'",
                            state.environment
                        ),
                        "run `labcoat state list` to see this environment's resources",
                    )
                })?;
                let instances: Vec<&labcoat_core::state::Instance> = if history {
                    record.instances.iter().collect()
                } else {
                    record
                        .active_instance
                        .as_deref()
                        .and_then(|id| record.instances.iter().find(|i| i.instance_id == id))
                        .into_iter()
                        .collect()
                };
                Ok(serde_json::json!({
                    "environment": state.environment,
                    "resource": address,
                    "kind": record.kind,
                    "activeInstance": record.active_instance,
                    "history": history,
                    "instances": instances,
                }))
            })();
            ("state-show", to_envelope(res))
        }
        StateCmd::Migrate => {
            let res = async {
                let network = ctx.config.network_id();
                let block1_hash = toolkit::chain_instance_id(&ctx.config).await;
                if block1_hash.is_none() {
                    ctx.warn(
                        "cannot reach the node — the chain identity will be backfilled by the next deploy against this environment",
                    );
                }
                let labcoat_network_instance_id = if ctx.config.network == NetworkTarget::Labcoat {
                    isomer_core::instance_id().ok()
                } else {
                    None
                };
                let chain = labcoat_core::state::ChainIdentity {
                    network: network.to_string(),
                    bitcoin_network: ctx.config.bitcoin_network_id().to_string(),
                    block1_hash,
                    labcoat_network_instance_id,
                };
                let now_millis = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let outcome = labcoat_core::state::migrate(
                    &cwd,
                    environment,
                    network,
                    chain,
                    labcoat_core::state::new_lineage(),
                    now_millis,
                )?;
                let display = |path: &std::path::Path| {
                    path.strip_prefix(&cwd)
                        .unwrap_or(path)
                        .display()
                        .to_string()
                };
                Ok(serde_json::json!({
                    "environment": environment,
                    "network": network,
                    "statePath": display(&outcome.state_path),
                    "backup": outcome.backup.as_deref().map(display),
                    "resources": outcome.resources,
                    "instances": outcome.instances,
                    "serial": outcome.serial,
                    "lineage": outcome.lineage,
                    "lockfileRegenerated": outcome.lockfile_regenerated,
                }))
            }
            .await;
            ("state-migrate", to_envelope(res))
        }
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
        assert_eq!(derived.contract_name, "counter");
        assert_eq!(derived.wasm_path, PathBuf::from("/tmp/counter.wasm"));

        let named = raw_deployment_artifact("/tmp/counter.wasm", Some("custom".into())).unwrap();
        assert_eq!(named.contract_name, "custom");
    }

    #[test]
    fn deploy_flags_select_steps_or_a_wasm_artifact_never_both() {
        // No flags: every pending step.
        assert!(matches!(
            DeployFlags::default().request().unwrap(),
            DeployRequest::Steps { only: None, replace } if replace.is_empty()
        ));
        // A name: that step alone.
        let named = DeployFlags {
            step: Some("counter".into()),
            ..DeployFlags::default()
        };
        assert!(matches!(
            named.request().unwrap(),
            DeployRequest::Steps { only: Some(only), .. } if only == "counter"
        ));
        // --wasm: the artifact, with the flags that describe it.
        let wasm = DeployFlags {
            wasm: Some("build/token.wasm".into()),
            name: Some("beta".into()),
            args: vec!["5000".into()],
            reserve: Some(7),
            ..DeployFlags::default()
        };
        assert!(matches!(
            wasm.request().unwrap(),
            DeployRequest::Wasm { name: Some(name), reserve: Some(7), .. } if name == "beta"
        ));

        for flags in [
            // A step's arguments, reserve, and transaction shape live in
            // deploy.rs.
            DeployFlags {
                step: Some("counter".into()),
                args: vec!["1".into()],
                ..DeployFlags::default()
            },
            DeployFlags {
                reserve: Some(7),
                ..DeployFlags::default()
            },
            DeployFlags {
                name: Some("beta".into()),
                ..DeployFlags::default()
            },
            DeployFlags {
                tx: TxFlags {
                    to: Some("bcrt1qexample".into()),
                    ..TxFlags::default()
                },
                ..DeployFlags::default()
            },
            // --wasm is outside deploy.rs: no step to name or replace.
            DeployFlags {
                wasm: Some("x.wasm".into()),
                step: Some("counter".into()),
                ..DeployFlags::default()
            },
            DeployFlags {
                wasm: Some("x.wasm".into()),
                replace: vec!["counter".into()],
                ..DeployFlags::default()
            },
        ] {
            assert_eq!(flags.request().unwrap_err().code, "CONFIG_INVALID");
        }
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
