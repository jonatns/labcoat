use labcoat_core::NetworkTarget;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const DEFAULT_NETWORK: &str = "labcoat";
pub const DEFAULT_RPC_URL: &str = "http://127.0.0.1:18443";
const DEFAULT_ENVIRONMENT: &str = "default";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectConfig {
    network: Option<String>,
    rpc_url: Option<String>,
    wallet_file: Option<PathBuf>,
    fee_rate: Option<f32>,
    signer: Option<String>,
    environment: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSettings {
    pub network: NetworkTarget,
    pub rpc_url: String,
    pub wallet_file: PathBuf,
    pub fee_rate: Option<f32>,
    /// Signing backend spec: `keystore` (default) or `psbt-file:<dir>`.
    pub signer: String,
    /// Durable-state environment (`.labcoat/state/<environment>/`).
    pub environment: String,
}

#[derive(Default)]
pub struct Overrides<'a> {
    pub network: Option<&'a str>,
    pub rpc_url: Option<&'a str>,
    pub wallet_file: Option<&'a str>,
    pub fee_rate: Option<f32>,
    pub signer: Option<&'a str>,
    pub environment: Option<&'a str>,
}

pub fn resolve(overrides: Overrides<'_>) -> Result<ResolvedSettings, String> {
    resolve_in(
        &std::env::current_dir().map_err(|e| e.to_string())?,
        overrides,
    )
}

fn resolve_in(root: &Path, overrides: Overrides<'_>) -> Result<ResolvedSettings, String> {
    resolve_in_with(root, overrides, |name| std::env::var(name).ok())
}

fn resolve_in_with(
    root: &Path,
    overrides: Overrides<'_>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<ResolvedSettings, String> {
    let config = load(root)?;
    let network_value = choose_string(
        overrides.network,
        env("LABCOAT_NETWORK"),
        config.network.clone(),
        DEFAULT_NETWORK,
    );
    let network = NetworkTarget::from_str(&network_value)?;
    // labcoat.toml describes one network. Its endpoint must not follow a
    // `--network` override to a different chain.
    let file_network = config
        .network
        .as_deref()
        .map_or(Ok(NetworkTarget::Labcoat), NetworkTarget::from_str)?;
    let rpc_url = choose_string(
        overrides.rpc_url,
        env("LABCOAT_RPC_URL"),
        config.rpc_url.filter(|_| file_network == network),
        default_rpc_url(network),
    );
    let rpc_url = if rpc_url == "subfrost" {
        subfrost_rpc_url(network)
            .ok_or_else(|| {
                format!(
                    "Subfrost has no endpoint for network '{}'; use signet, mainnet, or regtest",
                    network.id()
                )
            })?
            .to_string()
    } else {
        rpc_url
    };
    if network == NetworkTarget::Regtest && rpc_url == DEFAULT_RPC_URL {
        return Err(
            "network 'regtest' is reserved for custom regtest RPCs; use network = \"labcoat\" for Labcoat Network or configure a non-default rpc_url"
                .to_string(),
        );
    }
    let wallet_file = overrides
        .wallet_file
        .map(PathBuf::from)
        .or_else(|| env("LABCOAT_WALLET_FILE").map(PathBuf::from))
        .or(config.wallet_file)
        .unwrap_or_else(|| crate::local_wallet::default_path(network));
    if network != NetworkTarget::Labcoat && crate::local_wallet::is_built_in(&wallet_file) {
        return Err(format!(
            "the built-in Labcoat Network wallet cannot be used on {}; its passphrase is public. Create a wallet for this network with `labcoat --network {} wallet init`",
            network.id(),
            network.id()
        ));
    }
    let env_fee_rate = env("LABCOAT_FEE_RATE")
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<f32>()
                .map_err(|_| format!("invalid LABCOAT_FEE_RATE value `{value}`"))
        })
        .transpose()?;
    let fee_rate = overrides
        .fee_rate
        .or(env_fee_rate)
        .or(config.fee_rate)
        .or(Some(2.0));
    let signer = choose_string(
        overrides.signer,
        env("LABCOAT_SIGNER"),
        config.signer,
        "keystore",
    );
    let environment = choose_string(
        overrides.environment,
        env("LABCOAT_ENVIRONMENT"),
        config.environment,
        DEFAULT_ENVIRONMENT,
    );
    labcoat_core::state::validate_environment_name(&environment)
        .map_err(|e| format!("{}: {}", e.message, e.hint))?;

    Ok(ResolvedSettings {
        network,
        rpc_url,
        wallet_file,
        fee_rate,
        signer,
        environment,
    })
}

/// Subfrost's hosted Alkanes JSON-RPC for `network`.
fn subfrost_rpc_url(network: NetworkTarget) -> Option<&'static str> {
    match network {
        NetworkTarget::Signet => Some("https://signet.subfrost.io/v4/jsonrpc"),
        NetworkTarget::Mainnet => Some("https://mainnet.subfrost.io/v4/jsonrpc"),
        NetworkTarget::Regtest => Some("https://regtest.subfrost.io/v4/jsonrpc"),
        _ => None,
    }
}

/// Public networks default to Subfrost; everything else is a local node.
fn default_rpc_url(network: NetworkTarget) -> &'static str {
    match network {
        NetworkTarget::Signet | NetworkTarget::Mainnet => {
            subfrost_rpc_url(network).expect("Subfrost serves signet and mainnet")
        }
        _ => DEFAULT_RPC_URL,
    }
}

pub fn labcoat_network() -> ResolvedSettings {
    ResolvedSettings {
        network: NetworkTarget::Labcoat,
        rpc_url: DEFAULT_RPC_URL.to_string(),
        wallet_file: crate::local_wallet::default_path(NetworkTarget::Labcoat),
        fee_rate: Some(2.0),
        signer: "keystore".to_string(),
        environment: DEFAULT_ENVIRONMENT.to_string(),
    }
}

fn choose_string(
    cli: Option<&str>,
    env: Option<String>,
    file: Option<String>,
    default: &str,
) -> String {
    cli.map(str::to_owned)
        .or_else(|| env.filter(|v| !v.is_empty()))
        .or(file)
        .unwrap_or_else(|| default.to_owned())
}

fn load(root: &Path) -> Result<ProjectConfig, String> {
    let path = root.join("labcoat.toml");
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ProjectConfig::default()),
        Err(e) => return Err(format!("cannot read {}: {}", path.display(), e)),
    };
    toml::from_str(&raw).map_err(|e| {
        format!(
            "invalid {}: {} (allowed keys: network, rpc_url, wallet_file, fee_rate, signer, environment; secrets belong in LABCOAT_* env vars)",
            path.display(),
            e
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_is_cli_then_env_then_file_then_defaults() {
        let root = std::env::temp_dir().join(format!(
            "labcoat-settings-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("labcoat.toml"),
            "network = \"signet\"\nrpc_url = \"http://file\"\nfee_rate = 3.5\nsigner = \"psbt-file:./file-psbts\"\nenvironment = \"file-env\"\n",
        )
        .unwrap();

        let resolved = resolve_in_with(
            &root,
            Overrides {
                network: Some("regtest"),
                rpc_url: None,
                wallet_file: None,
                fee_rate: None,
                signer: None,
                environment: None,
            },
            |name| match name {
                "LABCOAT_NETWORK" => Some("mainnet".into()),
                "LABCOAT_RPC_URL" => Some("http://env".into()),
                "LABCOAT_WALLET_FILE" => Some("env-wallet.json".into()),
                "LABCOAT_SIGNER" => Some("psbt-file:./env-psbts".into()),
                "LABCOAT_ENVIRONMENT" => Some("env-env".into()),
                _ => None,
            },
        )
        .unwrap();
        assert_eq!(resolved.network, NetworkTarget::Regtest);
        assert_eq!(resolved.rpc_url, "http://env");
        assert_eq!(resolved.wallet_file, PathBuf::from("env-wallet.json"));
        assert_eq!(resolved.fee_rate, Some(3.5));
        assert_eq!(resolved.signer, "psbt-file:./env-psbts");
        assert_eq!(resolved.environment, "env-env");

        let from_file = resolve_in_with(
            &root,
            Overrides {
                network: Some("regtest"),
                rpc_url: Some("http://custom"),
                ..Overrides::default()
            },
            |_| None,
        )
        .unwrap();
        assert_eq!(from_file.environment, "file-env");

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn environment_defaults_and_is_validated_as_a_path_component() {
        let root = std::env::temp_dir().join(format!(
            "labcoat-settings-environment-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let resolved = resolve_in_with(&root, Overrides::default(), |_| None).unwrap();
        assert_eq!(resolved.environment, "default");

        let error = resolve_in_with(
            &root,
            Overrides {
                environment: Some("../escape"),
                ..Overrides::default()
            },
            |_| None,
        )
        .unwrap_err();
        assert!(error.contains("environment names must be non-empty"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn secrets_are_rejected_from_project_config() {
        let root =
            std::env::temp_dir().join(format!("labcoat-settings-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("labcoat.toml"), "mnemonic = \"never\"\n").unwrap();
        let error = load(&root).unwrap_err();
        assert!(error.contains("unknown field"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn defaults_to_labcoat_network() {
        let root =
            std::env::temp_dir().join(format!("labcoat-settings-defaults-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let resolved = resolve_in_with(&root, Overrides::default(), |_| None).unwrap();
        assert_eq!(resolved.network, NetworkTarget::Labcoat);
        assert_eq!(resolved.rpc_url, DEFAULT_RPC_URL);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn keystores_default_outside_the_project_and_the_built_in_one_stays_local() {
        let root =
            std::env::temp_dir().join(format!("labcoat-settings-wallets-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let on = |network, wallet_file: Option<&str>| {
            resolve_in_with(
                &root,
                Overrides {
                    network: Some(network),
                    wallet_file,
                    ..Overrides::default()
                },
                |_| None,
            )
        };
        let local = on("labcoat", None).unwrap().wallet_file;
        let signet = on("signet", None).unwrap().wallet_file;
        assert_ne!(local, signet);
        assert!(!local.starts_with(&root) && !signet.starts_with(&root));
        assert!(on("signet", local.to_str())
            .unwrap_err()
            .contains("built-in"));
        assert_eq!(
            on("signet", Some("mine.json")).unwrap().wallet_file,
            PathBuf::from("mine.json")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn public_networks_default_to_subfrost_and_never_inherit_the_local_endpoint() {
        let root =
            std::env::temp_dir().join(format!("labcoat-settings-subfrost-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("labcoat.toml"),
            "network = \"labcoat\"\nrpc_url = \"http://127.0.0.1:18443\"\n",
        )
        .unwrap();
        let rpc = |network: Option<&str>, rpc_url: Option<&str>| {
            resolve_in_with(
                &root,
                Overrides {
                    network,
                    rpc_url,
                    ..Overrides::default()
                },
                |_| None,
            )
            .map(|settings| settings.rpc_url)
        };
        assert_eq!(rpc(None, None).unwrap(), DEFAULT_RPC_URL);
        assert_eq!(
            rpc(Some("signet"), None).unwrap(),
            "https://signet.subfrost.io/v4/jsonrpc"
        );
        assert_eq!(
            rpc(Some("mainnet"), None).unwrap(),
            "https://mainnet.subfrost.io/v4/jsonrpc"
        );
        assert_eq!(
            rpc(Some("signet"), Some("http://mine")).unwrap(),
            "http://mine"
        );
        assert_eq!(
            rpc(Some("regtest"), Some("subfrost")).unwrap(),
            "https://regtest.subfrost.io/v4/jsonrpc"
        );
        assert!(rpc(Some("testnet"), Some("subfrost")).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn default_rpc_is_rejected_for_explicit_regtest() {
        let root =
            std::env::temp_dir().join(format!("labcoat-settings-regtest-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let error = resolve_in_with(
            &root,
            Overrides {
                network: Some("regtest"),
                ..Overrides::default()
            },
            |_| None,
        )
        .unwrap_err();
        assert!(error.contains("use network = \"labcoat\""));
        std::fs::remove_dir_all(root).ok();
    }
}
