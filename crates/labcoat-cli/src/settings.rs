use labcoat_core::NetworkTarget;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;

const DEFAULT_NETWORK: &str = "labcoat";
pub const DEFAULT_RPC_URL: &str = "http://127.0.0.1:18443";
const DEFAULT_WALLET_FILE: &str = ".labcoat/wallet.json";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectConfig {
    network: Option<String>,
    rpc_url: Option<String>,
    wallet_file: Option<PathBuf>,
    fee_rate: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSettings {
    pub network: NetworkTarget,
    pub rpc_url: String,
    pub wallet_file: PathBuf,
    pub fee_rate: Option<f32>,
}

#[derive(Default)]
pub struct Overrides<'a> {
    pub network: Option<&'a str>,
    pub rpc_url: Option<&'a str>,
    pub wallet_file: Option<&'a str>,
    pub fee_rate: Option<f32>,
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
        config.network,
        DEFAULT_NETWORK,
    );
    let network = NetworkTarget::from_str(&network_value)?;
    let rpc_url = choose_string(
        overrides.rpc_url,
        env("LABCOAT_RPC_URL"),
        config.rpc_url,
        DEFAULT_RPC_URL,
    );
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
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WALLET_FILE));
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

    Ok(ResolvedSettings {
        network,
        rpc_url,
        wallet_file,
        fee_rate,
    })
}

pub fn labcoat_network() -> ResolvedSettings {
    ResolvedSettings {
        network: NetworkTarget::Labcoat,
        rpc_url: DEFAULT_RPC_URL.to_string(),
        wallet_file: PathBuf::from(DEFAULT_WALLET_FILE),
        fee_rate: Some(2.0),
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
            "invalid {}: {} (allowed keys: network, rpc_url, wallet_file, fee_rate; secrets belong in LABCOAT_* env vars)",
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
            "network = \"signet\"\nrpc_url = \"http://file\"\nfee_rate = 3.5\n",
        )
        .unwrap();

        let resolved = resolve_in_with(
            &root,
            Overrides {
                network: Some("regtest"),
                rpc_url: None,
                wallet_file: None,
                fee_rate: None,
            },
            |name| match name {
                "LABCOAT_NETWORK" => Some("mainnet".into()),
                "LABCOAT_RPC_URL" => Some("http://env".into()),
                "LABCOAT_WALLET_FILE" => Some("env-wallet.json".into()),
                _ => None,
            },
        )
        .unwrap();
        assert_eq!(resolved.network, NetworkTarget::Regtest);
        assert_eq!(resolved.rpc_url, "http://env");
        assert_eq!(resolved.wallet_file, PathBuf::from("env-wallet.json"));
        assert_eq!(resolved.fee_rate, Some(3.5));

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
