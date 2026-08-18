//! End-to-end test context: drives contracts deployed on the developer's
//! Labcoat Network through the `labcoat` CLI's `--json` envelopes.
//!
//! Tests live in `tests/e2e.rs`, are marked `#[ignore]` (so plain
//! `cargo test` skips them), and run via `labcoat test --e2e`, which
//! resets the network, applies the `alkanes.hcl` manifest, and executes
//! them with the environment this context reads:
//!
//! ```text
//! LABCOAT_E2E_BIN   absolute path of the labcoat binary driving the run
//! LABCOAT_E2E_ROOT  the project root (where labcoat.lock lives)
//! ```
//!
//! ```no_run
//! use labcoat_test::e2e::{Call, E2e};
//!
//! #[test]
//! #[ignore = "network e2e — run with `labcoat test --e2e`"]
//! fn increments_on_chain() {
//!     let e2e = E2e::from_env().unwrap();
//!     let counter = e2e.contract("counter").unwrap();
//!     e2e.call(Call::new(&counter, "increment")).unwrap().success().unwrap();
//!     assert_eq!(e2e.simulate_uint(&counter, "get_count", &[]).unwrap(), 1);
//! }
//! ```

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Handle to a disposable, named test wallet (its own keystore file).
#[derive(Debug, Clone)]
pub struct Wallet {
    pub name: String,
    pub file: PathBuf,
    /// Primary p2tr receive address.
    pub address: String,
}

/// A state-changing contract call, built up fluently and executed with
/// [`E2e::call`].
#[derive(Debug, Clone)]
pub struct Call {
    contract: String,
    method: String,
    args: Vec<String>,
    inputs: Option<String>,
    to: Option<String>,
    pointer: Option<String>,
    refund: Option<String>,
    edicts: Vec<String>,
    wallet: Option<Wallet>,
}

/// A two-wallet, full-fill Alkane exchange executed as one transaction.
#[derive(Debug, Clone)]
pub struct Exchange {
    offered: String,
    offered_amount: u64,
    payment: String,
    payment_amount: u64,
    seller_wallet_file: PathBuf,
    buyer: Wallet,
}

impl Exchange {
    pub fn new(
        offered: &str,
        offered_amount: u64,
        payment: &str,
        payment_amount: u64,
        seller_wallet_file: impl Into<PathBuf>,
        buyer: &Wallet,
    ) -> Self {
        Self {
            offered: offered.to_string(),
            offered_amount,
            payment: payment.to_string(),
            payment_amount,
            seller_wallet_file: seller_wallet_file.into(),
            buyer: buyer.clone(),
        }
    }
}

impl Call {
    /// `contract` is a labcoat.lock name or `block:tx` id; `method` an ABI
    /// method name or decimal opcode.
    pub fn new(contract: &str, method: &str) -> Self {
        Self {
            contract: contract.to_string(),
            method: method.to_string(),
            args: Vec::new(),
            inputs: None,
            to: None,
            pointer: None,
            refund: None,
            edicts: Vec::new(),
            wallet: None,
        }
    }

    pub fn arg(mut self, value: impl ToString) -> Self {
        self.args.push(value.to_string());
        self
    }

    /// Extra transaction inputs: comma-separated alkanes `block:tx:amount`
    /// (amount 0 means all) or bitcoin `B:sats`.
    pub fn inputs(mut self, inputs: &str) -> Self {
        self.inputs = Some(inputs.to_string());
        self
    }

    /// Recipient of the protostone outputs (defaults to the signing
    /// wallet's primary address).
    pub fn to(mut self, address: &str) -> Self {
        self.to = Some(address.to_string());
        self
    }

    /// Pointer target `vN`/`pN` (default v0).
    pub fn pointer(mut self, target: &str) -> Self {
        self.pointer = Some(target.to_string());
        self
    }

    /// Refund target (defaults to the pointer target).
    pub fn refund(mut self, target: &str) -> Self {
        self.refund = Some(target.to_string());
        self
    }

    /// Edict `block:tx:amount:target` appended to the protostone.
    pub fn edict(mut self, edict: &str) -> Self {
        self.edicts.push(edict.to_string());
        self
    }

    /// Sign with a disposable test wallet instead of the project wallet.
    pub fn wallet(mut self, wallet: &Wallet) -> Self {
        self.wallet = Some(wallet.clone());
        self
    }
}

/// Result of a broadcast call.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub status: String,
    pub txid: String,
    pub revert_reason: Option<String>,
    /// The full result envelope, for anything not surfaced above.
    pub raw: Value,
}

impl Outcome {
    /// Error unless the call succeeded on-chain.
    pub fn success(self) -> Result<Outcome> {
        if self.status == "success" {
            Ok(self)
        } else {
            bail!(
                "call {} reverted: {}",
                self.txid,
                self.revert_reason.as_deref().unwrap_or("unknown reason")
            )
        }
    }

    /// The created contract's `block:tx` id (deploy outcomes).
    pub fn alkanes_id(&self) -> Result<String> {
        self.raw
            .get("alkanesId")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| anyhow!("outcome carries no alkanesId: {}", self.raw))
    }
}

/// The e2e test context. See the module docs.
pub struct E2e {
    bin: PathBuf,
    root: PathBuf,
}

impl E2e {
    /// Read the context `labcoat test --e2e` provides.
    pub fn from_env() -> Result<Self> {
        let bin = std::env::var_os("LABCOAT_E2E_BIN").ok_or_else(|| {
            anyhow!("LABCOAT_E2E_BIN is not set — run e2e tests with `labcoat test --e2e`")
        })?;
        let root = std::env::var_os("LABCOAT_E2E_ROOT").ok_or_else(|| {
            anyhow!("LABCOAT_E2E_ROOT is not set — run e2e tests with `labcoat test --e2e`")
        })?;
        Ok(Self {
            bin: PathBuf::from(bin),
            root: PathBuf::from(root),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run a labcoat subcommand and return the envelope's `result`.
    fn invoke(&self, wallet: Option<&Wallet>, args: &[&str]) -> Result<Value> {
        let mut command = Command::new(&self.bin);
        command.arg("--json").current_dir(&self.root);
        if let Some(wallet) = wallet {
            command.arg("--wallet-file").arg(&wallet.file);
        }
        command.args(args);
        let output = command
            .output()
            .with_context(|| format!("failed to run {}", self.bin.display()))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let envelope: Value = serde_json::from_str(stdout.trim()).with_context(|| {
            format!(
                "labcoat {} did not return a JSON envelope: {stdout}\n{}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
        if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
            let error = envelope.get("error").cloned().unwrap_or(Value::Null);
            bail!(
                "labcoat {} failed: [{}] {} ({})",
                args.join(" "),
                error.get("code").and_then(Value::as_str).unwrap_or("?"),
                error.get("message").and_then(Value::as_str).unwrap_or("?"),
                error.get("hint").and_then(Value::as_str).unwrap_or("")
            );
        }
        Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
    }

    /// The `block:tx` id recorded for a contract name in labcoat.lock.
    pub fn contract(&self, name: &str) -> Result<String> {
        let path = self.root.join("labcoat.lock");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {} — did apply run?", path.display()))?;
        let lockfile: Value = serde_json::from_str(&text)?;
        lockfile
            .get("networks")
            .and_then(|n| n.get("labcoat"))
            .and_then(|n| n.get(name))
            .and_then(|d| d.get("alkanesId"))
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| anyhow!("no deployment of `{name}` on labcoat in labcoat.lock"))
    }

    /// Broadcast a state-changing call and wait for its trace.
    pub fn call(&self, call: Call) -> Result<Outcome> {
        let mut args: Vec<String> = vec!["call".into(), call.contract.clone(), call.method.clone()];
        args.extend(call.args.iter().cloned());
        if let Some(inputs) = &call.inputs {
            args.extend(["--inputs".into(), inputs.clone()]);
        }
        if let Some(to) = &call.to {
            args.extend(["--to".into(), to.clone()]);
        }
        if let Some(pointer) = &call.pointer {
            args.extend(["--pointer".into(), pointer.clone()]);
        }
        if let Some(refund) = &call.refund {
            args.extend(["--refund".into(), refund.clone()]);
        }
        for edict in &call.edicts {
            args.extend(["--edict".into(), edict.clone()]);
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let result = self.invoke(call.wallet.as_ref(), &borrowed)?;
        Ok(Outcome {
            status: result
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            txid: result
                .get("txid")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            revert_reason: result
                .get("revertReason")
                .and_then(Value::as_str)
                .map(String::from),
            raw: result,
        })
    }

    /// Atomically deliver `offered` from the seller and `payment` from the
    /// buyer. The seller keystore is supplied explicitly; the buyer is the
    /// command's normal signing wallet.
    pub fn exchange(&self, exchange: Exchange) -> Result<Outcome> {
        let args = [
            "exchange".to_string(),
            exchange.offered,
            exchange.offered_amount.to_string(),
            exchange.payment,
            exchange.payment_amount.to_string(),
            "--seller-wallet-file".to_string(),
            exchange.seller_wallet_file.to_string_lossy().into_owned(),
        ];
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let result = self.invoke(Some(&exchange.buyer), &borrowed)?;
        Ok(Outcome {
            status: result
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            txid: result
                .get("txid")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            revert_reason: None,
            raw: result,
        })
    }

    /// Read-only simulation; returns the result envelope (status, decoded…).
    pub fn simulate(&self, contract: &str, method: &str, args: &[&str]) -> Result<Value> {
        let mut cli_args = vec!["simulate", contract, method];
        cli_args.extend_from_slice(args);
        self.invoke(None, &cli_args)
    }

    /// Simulate a method that returns a uint and decode it.
    pub fn simulate_uint(&self, contract: &str, method: &str, args: &[&str]) -> Result<u128> {
        let result = self.simulate(contract, method, args)?;
        if result.get("status").and_then(Value::as_str) != Some("success") {
            bail!("simulation of {method} failed: {result}");
        }
        result
            .get("decoded")
            .and_then(|d| d.get("uint"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow!("{method} did not decode to a uint: {result}"))
    }

    /// Alkanes token balance of `alkanes_id` (`block:tx`) held by `address`.
    pub fn balance(&self, address: &str, alkanes_id: &str) -> Result<u128> {
        let result = self.invoke(None, &["balance", address])?;
        let (block, tx) = alkanes_id
            .split_once(':')
            .ok_or_else(|| anyhow!("bad alkanes id `{alkanes_id}`; expected block:tx"))?;
        let (block, tx): (u64, u64) = (block.parse()?, tx.parse()?);
        let empty = Vec::new();
        let entries = result
            .get("balances")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        for entry in entries {
            let id = entry.get("alkane_id").cloned().unwrap_or(Value::Null);
            if id.get("block").and_then(Value::as_u64) == Some(block)
                && id.get("tx").and_then(Value::as_u64) == Some(tx)
            {
                return entry
                    .get("balance")
                    .and_then(|b| match b {
                        Value::Number(n) => n.as_u64().map(u128::from),
                        Value::String(s) => s.parse().ok(),
                        _ => None,
                    })
                    .ok_or_else(|| anyhow!("unreadable balance entry: {entry}"));
            }
        }
        Ok(0)
    }

    /// Current chain height.
    pub fn height(&self) -> Result<u64> {
        let result = self.invoke(None, &["status"])?;
        result
            .get("block_height")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("status did not report block_height: {result}"))
    }

    /// Mine `count` blocks (to the faucet's sink address).
    pub fn mine(&self, count: u64) -> Result<u64> {
        let result = self.invoke(None, &["mine", &count.to_string()])?;
        result
            .get("height")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("mine did not report a height: {result}"))
    }

    /// Mine until the chain reaches at least `height`.
    pub fn mine_until(&self, height: u64) -> Result<u64> {
        let current = self.height()?;
        if current >= height {
            return Ok(current);
        }
        self.mine(height - current)
    }

    /// Faucet-fund an address with regtest BTC and confirm it.
    pub fn fund(&self, address: &str, btc: f64) -> Result<()> {
        self.invoke(None, &["fund", address, &btc.to_string()])?;
        self.mine(1)?;
        Ok(())
    }

    /// A named disposable wallet under `.labcoat/e2e/`, created on first
    /// use. Whenever it holds no spendable UTXOs (fresh, or the chain was
    /// reset since), it is faucet-funded with `btc` regtest BTC.
    pub fn wallet(&self, name: &str, btc: f64) -> Result<Wallet> {
        let dir = self.root.join(".labcoat/e2e");
        std::fs::create_dir_all(&dir)?;
        let file = dir.join(format!("{name}.json"));
        let wallet_flag = file.to_string_lossy().to_string();
        if !file.exists() {
            let mut command = Command::new(&self.bin);
            command
                .args(["--json", "--wallet-file", &wallet_flag, "wallet", "init"])
                .current_dir(&self.root);
            let output = command.output()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let envelope: Value = serde_json::from_str(stdout.trim())
                .with_context(|| format!("wallet init did not return an envelope: {stdout}"))?;
            if envelope.get("ok").and_then(Value::as_bool) != Some(true) {
                bail!("wallet init for `{name}` failed: {envelope}");
            }
        }
        let wallet = Wallet {
            name: name.to_string(),
            file: file.clone(),
            address: String::new(),
        };
        let addresses = self.invoke(Some(&wallet), &["wallet", "addresses"])?;
        let address = addresses
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("address"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("wallet `{name}` reported no addresses"))?
            .to_string();
        let wallet = Wallet {
            name: name.to_string(),
            file,
            address,
        };
        if btc > 0.0 {
            // A keystore can outlive a chain reset; fund on actual balance,
            // not file existence.
            let utxos = self.invoke(Some(&wallet), &["wallet", "utxos"])?;
            if utxos.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                self.fund(&wallet.address, btc)?;
            }
        }
        Ok(wallet)
    }

    /// The project wallet's primary address (the manifest's signer).
    pub fn project_address(&self) -> Result<String> {
        let addresses = self.invoke(None, &["wallet", "addresses"])?;
        addresses
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.get("address"))
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| anyhow!("project wallet reported no addresses"))
    }

    /// Broadcast a deploy outside the manifest (e.g. signed by a
    /// disposable wallet) and record it in labcoat.lock.
    pub fn deploy(&self, deploy: Deploy) -> Result<Outcome> {
        let mut args: Vec<String> = vec!["deploy".into()];
        match &deploy.source {
            DeploySource::Package(package) => args.push(package.clone()),
            DeploySource::Wasm { path, name } => {
                args.extend(["--wasm".into(), path.clone()]);
                args.extend(["--name".into(), name.clone()]);
            }
        }
        if let Some(reserve) = deploy.reserve {
            args.extend(["--reserve".into(), reserve.to_string()]);
        }
        if !deploy.args.is_empty() {
            args.push("--args".into());
            args.push(deploy.args.join(","));
        }
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let result = self.invoke(deploy.wallet.as_ref(), &borrowed)?;
        Ok(Outcome {
            status: result
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            txid: result
                .get("txid")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            revert_reason: result
                .get("revertReason")
                .and_then(Value::as_str)
                .map(String::from),
            raw: result,
        })
    }
}

enum DeploySource {
    Package(String),
    Wasm { path: String, name: String },
}

/// A contract deployment, built up fluently and executed with
/// [`E2e::deploy`].
pub struct Deploy {
    source: DeploySource,
    reserve: Option<u128>,
    args: Vec<String>,
    wallet: Option<Wallet>,
}

impl Deploy {
    /// Deploy a Cargo contract package (the package name becomes the
    /// lockfile name; use [`Deploy::wasm`] to deploy one artifact under
    /// several names).
    pub fn package(package: &str) -> Self {
        Self {
            source: DeploySource::Package(package.to_string()),
            reserve: None,
            args: Vec::new(),
            wallet: None,
        }
    }

    /// Deploy a prebuilt raw wasm artifact under the given lockfile name.
    pub fn wasm(path: &str, name: &str) -> Self {
        Self {
            source: DeploySource::Wasm {
                path: path.to_string(),
                name: name.to_string(),
            },
            reserve: None,
            args: Vec::new(),
            wallet: None,
        }
    }

    /// Deploy to reserved number N (cellpack target `[3,N]`).
    pub fn reserve(mut self, reserve: u128) -> Self {
        self.reserve = Some(reserve);
        self
    }

    /// One constructor argument per ABI constructor parameter.
    pub fn arg(mut self, value: impl ToString) -> Self {
        self.args.push(value.to_string());
        self
    }

    /// Sign with a disposable test wallet instead of the project wallet.
    pub fn wallet(mut self, wallet: &Wallet) -> Self {
        self.wallet = Some(wallet.clone());
        self
    }
}
