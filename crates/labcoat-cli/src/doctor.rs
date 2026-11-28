//! `labcoat doctor` — environment diagnosis: toolchain, ports, disk,
//! binaries, project state. Read-only; every check reports pass/warn/fail
//! with a fix-it hint.

use isomer_core::{BinaryManager, IsomerConfig, ProcessManager, ServiceId};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub status: &'static str, // "ok" | "warn" | "fail"
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

fn ok(name: &str, detail: impl Into<String>) -> Check {
    Check {
        name: name.to_string(),
        status: "ok",
        detail: detail.into(),
        hint: None,
    }
}

fn warn(name: &str, detail: impl Into<String>, hint: impl Into<String>) -> Check {
    Check {
        name: name.to_string(),
        status: "warn",
        detail: detail.into(),
        hint: Some(hint.into()),
    }
}

fn fail(name: &str, detail: impl Into<String>, hint: impl Into<String>) -> Check {
    Check {
        name: name.to_string(),
        status: "fail",
        detail: detail.into(),
        hint: Some(hint.into()),
    }
}

fn version_of(cmd: &str, arg: &str) -> Option<String> {
    std::process::Command::new(cmd)
        .arg(arg)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
}

/// The nearest ancestor of `cwd` that holds `labcoat.toml`.
fn project_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|dir| dir.join("labcoat.toml").is_file())
        .map(Path::to_path_buf)
}

/// The `[toolchain] channel` of a `rust-toolchain.toml`, if the file exists
/// and names one.
fn toolchain_channel(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    value
        .get("toolchain")?
        .get("channel")?
        .as_str()
        .map(str::to_owned)
}

/// Whether rustup already has `channel`; `None` when rustup cannot answer.
/// Listing toolchains never installs one, unlike running cargo through a
/// pinned project.
fn rustup_toolchain_installed(channel: &str) -> Option<bool> {
    let output = std::process::Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let prefix = format!("{channel}-");
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .any(|name| name == channel || name.starts_with(&prefix)),
    )
}

/// Whether the project's alkanes-rs, metashrew, and labcoat-test pins match
/// the installed CLI. The toolchain pin has its own check.
fn project_pins_check(root: &Path) -> Check {
    const NAME: &str = "project pins";
    match crate::upgrade::plan(root) {
        Ok(report) if report.managed == 0 => warn(
            NAME,
            "Cargo.toml has no alkanes-rs or labcoat-test git dependencies",
            "compare Cargo.toml with a project from `labcoat init`",
        ),
        Ok(report) => {
            let stale: Vec<String> = report
                .changes
                .iter()
                .filter(|change| change.file == "Cargo.toml")
                .map(|change| {
                    format!(
                        "{} ({} → {})",
                        change.item,
                        change.from.as_deref().unwrap_or("unpinned"),
                        change.to
                    )
                })
                .collect();
            if stale.is_empty() {
                ok(
                    NAME,
                    format!(
                        "alkanes-rs and labcoat-test match Labcoat {}",
                        env!("CARGO_PKG_VERSION")
                    ),
                )
            } else {
                warn(
                    NAME,
                    format!(
                        "pinned for a different Labcoat release: {}",
                        stale.join(", ")
                    ),
                    "run `labcoat upgrade` (preview with --dry-run), then `labcoat test`",
                )
            }
        }
        Err(error) => warn(NAME, error.message, error.hint),
    }
}

/// Whether the project's `labcoat.toml` can be used. Every other command
/// stops on a broken file; this one says what is wrong with it and goes on
/// to check the toolchain and runtime.
fn project_config_check(root: &Path) -> Check {
    const NAME: &str = "project config";
    let path = root.join("labcoat.toml");
    match crate::settings::resolve_at(root) {
        Ok(settings) => ok(
            NAME,
            format!(
                "{} targets {} at {}",
                path.display(),
                settings.network.id(),
                settings.rpc_url
            ),
        ),
        Err(error) => fail(
            NAME,
            error,
            "fix labcoat.toml; allowed keys are network, rpc_url, wallet_file, fee_rate, signer, and environment",
        ),
    }
}

/// Whether this release's runtime is installed. A runtime that has not been
/// downloaded yet is how every machine starts, and the first `labcoat up`
/// fetches it: that is a note, not a problem. One that is present but does
/// not verify is a problem.
fn runtime_bundle_check<'a>(
    statuses: impl Iterator<Item = &'a isomer_core::BinaryStatus>,
    release_tag: &str,
) -> Check {
    const NAME: &str = "runtime bundle";
    let mut missing = false;
    let mut invalid = Vec::new();
    for status in statuses {
        match status {
            isomer_core::BinaryStatus::Installed { .. } => {}
            isomer_core::BinaryStatus::NotInstalled => missing = true,
            isomer_core::BinaryStatus::Invalid { reason } => invalid.push(reason.clone()),
        }
    }
    if !invalid.is_empty() {
        fail(
            NAME,
            format!(
                "{release_tag} is installed but invalid: {}",
                invalid.join("; ")
            ),
            "run `labcoat binaries --download` to replace it",
        )
    } else if missing {
        warn(
            NAME,
            format!("{release_tag} is not installed yet"),
            "run `labcoat up`; it downloads the runtime the first time",
        )
    } else {
        ok(NAME, format!("{release_tag} is installed"))
    }
}

/// Who holds Labcoat Network's port, said the way `labcoat up` would say
/// it: this data home's network, another data home's, or something else.
fn port_check(
    port: u16,
    free: bool,
    identify: impl FnOnce() -> isomer_core::rpc::NodeIdentity,
) -> Check {
    use isomer_core::rpc::NodeIdentity;
    const NAME: &str = "ports";
    if free {
        return ok(NAME, format!("{port} is free"));
    }
    match identify() {
        NodeIdentity::Own => ok(NAME, format!("Labcoat Network is running on {port}")),
        NodeIdentity::OwnOtherRelease => warn(
            NAME,
            format!("{port} serves this data directory's network from another Labcoat release"),
            "run `labcoat up`; it replaces it with this release's",
        ),
        NodeIdentity::OtherDataHome(data_dir) => warn(
            NAME,
            format!("{port} serves a Labcoat Network for another data directory ({data_dir})"),
            "stop it with `labcoat down` under that LABCOAT_DATA_HOME, or set LABCOAT_RPC_PORT to run this one on another port",
        ),
        _ => warn(
            NAME,
            format!("{port} is in use by another program"),
            "stop the other program, or set LABCOAT_RPC_PORT to run Labcoat Network on another port",
        ),
    }
}

/// Whether the running network signs for its built-in wallet. The methods
/// are always served; what can be missing is the wallet `labcoat up` makes.
fn dev_signer_check(signer: &isomer_core::DevSignerInfo) -> Check {
    const NAME: &str = "dev signer";
    match (&signer.address, &signer.reason) {
        (Some(address), _) if signer.available => ok(
            NAME,
            format!(
                "signs PSBTs for the built-in wallet {address} at {}",
                signer.rpc
            ),
        ),
        (_, reason) => warn(
            NAME,
            format!(
                "not available: {}",
                reason.as_deref().unwrap_or("no answer")
            ),
            "run `labcoat up`; it creates and funds the built-in wallet",
        ),
    }
}

fn project_toolchain_check(pinned: Option<&str>, expected: &str, installed: Option<bool>) -> Check {
    const NAME: &str = "project toolchain";
    match pinned {
        None => warn(
            NAME,
            "no rust-toolchain.toml; contracts build with your default Rust toolchain",
            format!("run `labcoat upgrade` to pin Rust {expected} and the WebAssembly targets"),
        ),
        Some(channel) if channel != expected => warn(
            NAME,
            format!("project pins Rust {channel}; this Labcoat release is tested with {expected}"),
            format!("set channel = \"{expected}\" in rust-toolchain.toml, then run `labcoat test`"),
        ),
        Some(channel) => ok(
            NAME,
            match installed {
                Some(true) => format!("Rust {channel} (installed)"),
                Some(false) => format!("Rust {channel} (rustup installs it on the first build)"),
                None => format!("Rust {channel}"),
            },
        ),
    }
}

/// Whether the selected network's endpoint will serve a deploy. Subfrost's
/// public endpoints refuse a caller without an API key past 20 requests a
/// minute, which a deploy exceeds.
fn subfrost_key_check(
    target: &crate::settings::ResolvedSettings,
    key: Option<&str>,
) -> Option<Check> {
    const NAME: &str = "subfrost api key";
    if target.network.uses_regtest() || !labcoat_core::system::is_subfrost(&target.rpc_url) {
        return None;
    }
    Some(if key.is_some_and(|key| !key.trim().is_empty()) {
        ok(
            NAME,
            format!("SUBFROST_API_KEY is set for {}", target.network.id()),
        )
    } else {
        warn(
            NAME,
            format!(
                "SUBFROST_API_KEY is not set; Subfrost's {} endpoint allows 20 requests a minute without one, fewer than a deploy makes",
                target.network.id()
            ),
            "get a key at https://api.subfrost.io and set SUBFROST_API_KEY",
        )
    })
}

/// `target` is what the command line and the project resolve to, when
/// they resolve.
pub async fn run(target: Option<&crate::settings::ResolvedSettings>) -> Vec<Check> {
    let mut checks = Vec::new();

    // Toolchain
    match version_of("cargo", "--version") {
        Some(v) => checks.push(ok("cargo", v)),
        None => checks.push(fail(
            "cargo",
            "not found on PATH",
            "install Rust via rustup (contract compilation needs cargo)",
        )),
    }
    let rustup = version_of("rustup", "--version");
    match &rustup {
        Some(v) => checks.push(ok("rustup", v.clone())),
        None => checks.push(warn(
            "rustup",
            "not found on PATH",
            "install rustup from https://rustup.rs; only rustup applies a project's rust-toolchain.toml",
        )),
    }
    let project = project_root(&std::env::current_dir().unwrap_or_default());
    if let Some(root) = project.as_deref() {
        checks.push(project_config_check(root));
    }
    if let Some(target) = target {
        let key = std::env::var("SUBFROST_API_KEY").ok();
        checks.extend(subfrost_key_check(target, key.as_deref()));
    }
    let pinned = project
        .as_deref()
        .map(|root| toolchain_channel(&root.join("rust-toolchain.toml")));
    let mut wasm_target_pending = false;
    if let Some(pinned) = pinned {
        let installed = match (&pinned, &rustup) {
            (Some(channel), Some(_)) => rustup_toolchain_installed(channel),
            _ => None,
        };
        wasm_target_pending = installed == Some(false);
        checks.push(project_toolchain_check(
            pinned.as_deref(),
            labcoat_core::compile::PROJECT_RUST_TOOLCHAIN,
            installed,
        ));
    }
    if let Some(root) = project.as_deref() {
        checks.push(project_pins_check(root));
    }
    if wasm_target_pending {
        checks.push(ok(
            "wasm32-unknown-unknown",
            "installed with the project toolchain on the first build",
        ));
    } else {
        let wasm_target = std::process::Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-unknown-unknown"))
            .unwrap_or(false);
        if wasm_target {
            checks.push(ok("wasm32-unknown-unknown", "target installed"));
        } else {
            checks.push(warn(
                "wasm32-unknown-unknown",
                "target not reported by rustup",
                "rustup target add wasm32-unknown-unknown",
            ));
        }
    }
    match labcoat_core::compile::wasm_c_compiler() {
        Some(path) => checks.push(ok(
            "wasm C compiler",
            format!("{} supports wasm32", path.display()),
        )),
        None => checks.push(fail(
            "wasm C compiler",
            "no LLVM clang with a wasm32 backend found",
            "install LLVM (`brew install llvm` on macOS, `apt install clang wasi-libc` on Debian or Ubuntu)",
        )),
    }
    // The Alkanes support crates every project depends on generate code
    // with it at build time.
    match std::env::var("PROTOC")
        .ok()
        .filter(|path| std::path::Path::new(path).is_file())
        .or_else(|| version_of("protoc", "--version"))
    {
        Some(found) => checks.push(ok("protoc", found)),
        None => checks.push(fail(
            "protoc",
            "the Protocol Buffers compiler was not found on PATH",
            "install it (`brew install protobuf` on macOS, `apt install protobuf-compiler` on Debian or Ubuntu); contracts do not build without it",
        )),
    }
    // Port
    let config = IsomerConfig::load();
    let port = ProcessManager::port_for_service(ServiceId::Node, &config);
    checks.push(port_check(
        port,
        std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
        || isomer_core::rpc::identify_blocking(&config, std::time::Duration::from_secs(2)),
    ));

    // The dev signer, when this data home's network is the one running.
    if isomer_core::rpc::is_own_node(&config).await {
        checks.push(dev_signer_check(
            &isomer_core::rpc::dev_signer(&config).await,
        ));
    }

    // Binaries
    let infos = BinaryManager::new().check_all();
    checks.push(runtime_bundle_check(
        infos.iter().map(|binary| &binary.status),
        &BinaryManager::release_tag(),
    ));

    // Disk space in the data dir
    let data_dir = isomer_core::get_data_dir();
    let probe = data_dir.join(".doctor-probe");
    let disk_writable =
        std::fs::create_dir_all(&data_dir).is_ok() && std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    if disk_writable {
        checks.push(ok(
            "data dir",
            format!("{} is writable", data_dir.display()),
        ));
    } else {
        checks.push(fail(
            "data dir",
            format!("{} is not writable", data_dir.display()),
            "check permissions/disk space",
        ));
    }

    // Project state
    if labcoat_core::lockfile::path(&crate::project::root()).exists() {
        checks.push(ok("labcoat.lock", "present"));
    }
    match crate::settings::resolve(crate::settings::Overrides::default()) {
        Ok(settings) if settings.wallet_file.exists() => checks.push(ok(
            "wallet",
            format!("{} present", settings.wallet_file.display()),
        )),
        Ok(settings) => checks.push(warn(
            "wallet",
            format!("no keystore at {}", settings.wallet_file.display()),
            if crate::local_wallet::is_built_in(&settings.wallet_file) {
                "labcoat up"
            } else {
                "labcoat wallet init"
            },
        )),
        // The project config check has already reported why.
        Err(_) if project.is_some() => {}
        Err(message) => checks.push(warn("wallet", message, "fix the LABCOAT_* settings")),
    }

    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_public_subfrost_target_is_checked_for_an_api_key() {
        let target = |network, rpc_url: &str| crate::settings::ResolvedSettings {
            network,
            rpc_url: rpc_url.to_string(),
            wallet_file: PathBuf::from("wallet.json"),
            fee_rate: None,
            signer: "keystore".into(),
            environment: "default".into(),
        };
        let signet = target(
            labcoat_core::NetworkTarget::Signet,
            "https://signet.subfrost.io/v4/jsonrpc",
        );
        let missing = subfrost_key_check(&signet, None).unwrap();
        assert_eq!(missing.status, "warn");
        assert!(missing.detail.contains("20 requests a minute"));
        assert!(missing.hint.unwrap().contains("SUBFROST_API_KEY"));
        assert_eq!(
            subfrost_key_check(&signet, Some("")).unwrap().status,
            "warn"
        );
        assert_eq!(
            subfrost_key_check(&signet, Some("sk-test")).unwrap().status,
            "ok"
        );

        // Nothing to say about Labcoat Network or another provider.
        let local = target(
            labcoat_core::NetworkTarget::Labcoat,
            "http://127.0.0.1:18443",
        );
        assert!(subfrost_key_check(&local, None).is_none());
        let own = target(
            labcoat_core::NetworkTarget::Signet,
            "https://node.example/rpc",
        );
        assert!(subfrost_key_check(&own, None).is_none());
    }

    #[test]
    fn project_toolchain_check_reports_missing_matching_and_stale_pins() {
        let missing = project_toolchain_check(None, "1.100.0", None);
        assert_eq!(missing.status, "warn");
        assert!(missing.hint.unwrap().contains("1.100.0"));

        let stale = project_toolchain_check(Some("1.88.0"), "1.100.0", Some(true));
        assert_eq!(stale.status, "warn");
        assert!(stale.detail.contains("1.88.0"));
        assert!(stale.hint.unwrap().contains("channel = \"1.100.0\""));

        let pending = project_toolchain_check(Some("1.100.0"), "1.100.0", Some(false));
        assert_eq!(pending.status, "ok");
        assert!(pending.detail.contains("first build"));
    }

    #[test]
    fn a_broken_labcoat_toml_is_a_failed_check_not_a_stopped_command() {
        let root =
            std::env::temp_dir().join(format!("labcoat-doctor-config-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("labcoat.toml"), "network = \"labcoat\"\n").unwrap();
        let valid = project_config_check(&root);
        assert_eq!(valid.status, "ok");
        assert!(valid
            .detail
            .contains("targets labcoat at http://127.0.0.1:"));

        std::fs::write(root.join("labcoat.toml"), "network = [not toml\n").unwrap();
        let broken = project_config_check(&root);
        assert_eq!(broken.status, "fail");
        assert!(broken.detail.contains("labcoat.toml"));

        std::fs::write(root.join("labcoat.toml"), "mnemonic = \"never\"\n").unwrap();
        assert_eq!(project_config_check(&root).status, "fail");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_dev_signer_check_passes_with_a_wallet_and_warns_without_one() {
        let mut signer = isomer_core::DevSignerInfo {
            available: true,
            rpc: "http://127.0.0.1:18443".into(),
            methods: vec![],
            address: Some("bcrt1pexample".into()),
            reason: None,
        };
        let ready = dev_signer_check(&signer);
        assert_eq!(ready.status, "ok");
        assert!(ready
            .detail
            .contains("bcrt1pexample at http://127.0.0.1:18443"));

        signer.available = false;
        signer.address = None;
        signer.reason = Some("the built-in wallet does not exist yet".into());
        let missing = dev_signer_check(&signer);
        assert_eq!(missing.status, "warn");
        assert!(missing.detail.contains("does not exist yet"));
        assert_eq!(
            missing.hint.unwrap(),
            "run `labcoat up`; it creates and funds the built-in wallet"
        );
    }

    #[test]
    fn the_port_check_names_who_holds_the_port() {
        use isomer_core::rpc::NodeIdentity;
        assert_eq!(port_check(18443, true, || unreachable!()).status, "ok");
        let own = port_check(18443, false, || NodeIdentity::Own);
        assert_eq!(own.status, "ok");
        assert!(own.detail.contains("running on 18443"));

        let other = port_check(18443, false, || {
            NodeIdentity::OtherDataHome("/elsewhere/network".into())
        });
        assert_eq!(other.status, "warn");
        assert!(other.detail.contains("/elsewhere/network"));
        assert!(other.hint.unwrap().contains("LABCOAT_RPC_PORT"));

        let foreign = port_check(18443, false, || NodeIdentity::Foreign);
        assert!(foreign.detail.contains("another program"));
    }

    #[test]
    fn a_runtime_that_is_not_downloaded_yet_is_not_a_problem() {
        use isomer_core::BinaryStatus;
        let missing = runtime_bundle_check([BinaryStatus::NotInstalled].iter(), "cli-v1.2.3");
        assert_eq!(missing.status, "warn");
        assert!(missing.hint.unwrap().contains("labcoat up"));

        let invalid = runtime_bundle_check(
            [BinaryStatus::Invalid {
                reason: "checksum mismatch".into(),
            }]
            .iter(),
            "cli-v1.2.3",
        );
        assert_eq!(invalid.status, "fail");
        assert!(invalid.detail.contains("checksum mismatch"));
        assert!(invalid.hint.unwrap().contains("binaries --download"));
    }

    #[test]
    fn toolchain_channel_reads_the_pinned_channel() {
        let dir = std::env::temp_dir().join(format!("labcoat-doctor-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("labcoat.toml"), "").unwrap();
        let file = dir.join("rust-toolchain.toml");
        std::fs::write(&file, "[toolchain]\nchannel = \"1.100.0\"\n").unwrap();
        assert_eq!(toolchain_channel(&file).as_deref(), Some("1.100.0"));
        assert_eq!(
            project_root(&dir.join("nested")).as_deref(),
            Some(dir.as_path())
        );
        assert_eq!(toolchain_channel(&dir.join("missing.toml")), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
