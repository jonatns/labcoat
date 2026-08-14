//! Provider bootstrap against the pinned alkanes-rs main commit.
//!
//! Mirrors what alkanes-cli's `SystemAlkanes::new_with_options` does for
//! our fixed shape (Qubitcoin RPC endpoint, project-local keystore),
//! without depending on `alkanes-cli-sys` (broken at the pinned rev) or
//! clap `Args`.

use crate::error::{LabcoatError, Result};
use alkanes_cli_common::provider::ConcreteProvider;
use std::path::PathBuf;
use std::str::FromStr;

/// A user-facing Labcoat deployment target.
///
/// `Labcoat` is the managed local network identity. It deliberately maps to
/// Bitcoin regtest only at protocol boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkTarget {
    Labcoat,
    Regtest,
    Signet,
    Testnet,
    Mainnet,
}

impl NetworkTarget {
    pub const ALLOWED: &'static str = "labcoat, regtest, signet, testnet, mainnet";

    pub fn id(self) -> &'static str {
        match self {
            Self::Labcoat => "labcoat",
            Self::Regtest => "regtest",
            Self::Signet => "signet",
            Self::Testnet => "testnet",
            Self::Mainnet => "mainnet",
        }
    }

    /// The Bitcoin network parameters used by the provider and wallet.
    pub fn bitcoin_network_id(self) -> &'static str {
        match self {
            Self::Labcoat | Self::Regtest => "regtest",
            Self::Signet => "signet",
            Self::Testnet => "testnet",
            Self::Mainnet => "mainnet",
        }
    }

    pub fn uses_regtest(self) -> bool {
        self.bitcoin_network_id() == "regtest"
    }
}

impl Default for NetworkTarget {
    fn default() -> Self {
        Self::Labcoat
    }
}

impl std::fmt::Display for NetworkTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.id())
    }
}

impl FromStr for NetworkTarget {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "labcoat" => Ok(Self::Labcoat),
            "regtest" => Ok(Self::Regtest),
            "signet" => Ok(Self::Signet),
            "testnet" => Ok(Self::Testnet),
            "mainnet" => Ok(Self::Mainnet),
            "oylnet" => {
                tracing::warn!("network 'oylnet' is deprecated; treating as 'regtest'");
                Ok(Self::Regtest)
            }
            other => Err(format!(
                "unknown network '{}'; use one of: {}",
                other,
                Self::ALLOWED
            )),
        }
    }
}

/// Connection + wallet settings for the toolkit.
#[derive(Debug, Clone)]
pub struct ToolkitConfig {
    /// Deployment target. Labcoat Network uses Bitcoin regtest underneath.
    pub network: NetworkTarget,
    /// Direct Qubitcoin RPC endpoint.
    pub rpc_url: String,
    /// Keystore path (project-local by default).
    pub wallet_file: PathBuf,
    /// Fee rate in sat/vB for state-changing operations.
    pub fee_rate: Option<f32>,
}

impl Default for ToolkitConfig {
    fn default() -> Self {
        Self {
            network: NetworkTarget::Labcoat,
            rpc_url: "http://127.0.0.1:18443".to_string(),
            wallet_file: PathBuf::from(".labcoat/wallet.json"),
            fee_rate: Some(2.0),
        }
    }
}

impl ToolkitConfig {
    pub fn network_id(&self) -> &'static str {
        self.network.id()
    }

    pub fn bitcoin_network_id(&self) -> &'static str {
        self.network.bitcoin_network_id()
    }

    /// Refuse footgun setups: mainnet/signet require an explicit passphrase.
    pub fn require_passphrase_policy(&self, passphrase: &Option<String>) -> Result<()> {
        let net = self.network_id();
        if passphrase.is_none() && (net == "mainnet" || net == "signet") {
            return Err(LabcoatError::new(
                "WALLET_LOCKED",
                format!("a wallet passphrase is required on {net}"),
                "set LABCOAT_WALLET_PASSPHRASE",
            ));
        }
        Ok(())
    }
}

/// Build a ready `ConcreteProvider`. When `wallet_needed` is true the
/// keystore at `wallet_file` is loaded (and unlocked if a passphrase is
/// given); read-only commands skip that.
pub async fn connect(
    config: &ToolkitConfig,
    passphrase: Option<String>,
    wallet_needed: bool,
) -> Result<ConcreteProvider> {
    let network = config.bitcoin_network_id();

    // Network params + process-global network (address derivation/signing).
    let params =
        alkanes_cli_common::network::NetworkParams::from_network_str(network).map_err(|e| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!("unknown Bitcoin network '{network}': {e}"),
                "use one of: labcoat, regtest, signet, testnet, mainnet",
            )
        })?;
    alkanes_cli_common::network::set_network(params);

    // The keystore writer assumes its parent directory exists.
    if let Some(parent) = config.wallet_file.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                LabcoatError::new(
                    "TOOLKIT_ERROR",
                    format!("cannot create {}: {}", parent.display(), e),
                    "check permissions on the project directory",
                )
            })?;
        }
    }

    let mut provider = ConcreteProvider::new_with_headers(
        Some(config.rpc_url.clone()),
        config.rpc_url.clone(),
        None,
        None,
        None,
        None,
        network.to_string(),
        Some(config.wallet_file.clone()),
        Vec::new(),
    )
    .await
    .map_err(|e| LabcoatError::classify(e.into()))?;
    provider.rpc_config.qubitcoin_rpc_url = Some(config.rpc_url.clone());
    // Upstream's legacy URL selector does not consider qubitcoin_rpc_url for
    // Esplora/Ord commands. Point its JSON-RPC fallback at the same direct
    // Qubitcoin endpoint so `call()` can apply the secondaryview translation.
    provider.rpc_config.jsonrpc_url = Some(config.rpc_url.clone());

    // In-memory cache: deterministic, no ~/.alkanes/cache.sqlite3 side state.
    provider = provider.with_cache(std::sync::Arc::new(
        alkanes_cli_common::cache::in_memory::InMemoryCache::new(),
    ));

    provider.set_passphrase(passphrase);

    if wallet_needed {
        ConcreteProvider::initialize(&mut provider)
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
    }

    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alkanes_cli_common::commands::{Commands, EsploraCommands};
    use alkanes_cli_common::rpc::{determine_rpc_call_type, get_rpc_url, RpcCallType};

    #[tokio::test]
    async fn provider_routes_legacy_esplora_commands_to_qubitcoin() {
        let config = ToolkitConfig::default();
        let provider = connect(&config, None, false).await.unwrap();
        let tip_height = Commands::Esplora {
            command: EsploraCommands::BlocksTipHeight { raw: false },
        };

        assert_eq!(
            provider.rpc_config.qubitcoin_rpc_url.as_deref(),
            Some("http://127.0.0.1:18443")
        );
        assert_eq!(
            provider.rpc_config.jsonrpc_url.as_deref(),
            provider.rpc_config.qubitcoin_rpc_url.as_deref()
        );
        assert!(provider.rpc_config.esplora_url.is_none());
        assert!(provider.rpc_config.is_qubitcoin_mode());
        assert_eq!(
            get_rpc_url(&provider.rpc_config, &tip_height).unwrap(),
            "http://127.0.0.1:18443"
        );
        assert_eq!(
            determine_rpc_call_type(&provider.rpc_config, &tip_height),
            RpcCallType::JsonRpc
        );
    }

    #[test]
    fn toolkit_defaults_to_local_qubitcoin_rpc() {
        assert_eq!(ToolkitConfig::default().rpc_url, "http://127.0.0.1:18443");
    }

    #[test]
    fn labcoat_target_maps_to_regtest_only_at_protocol_boundaries() {
        let config = ToolkitConfig::default();
        assert_eq!(config.network_id(), "labcoat");
        assert_eq!(config.bitcoin_network_id(), "regtest");
        assert!(config.network.uses_regtest());
    }

    #[test]
    fn parses_supported_targets_and_deprecated_oylnet() {
        assert_eq!(
            NetworkTarget::from_str("labcoat").unwrap(),
            NetworkTarget::Labcoat
        );
        assert_eq!(
            NetworkTarget::from_str("regtest").unwrap(),
            NetworkTarget::Regtest
        );
        assert_eq!(
            NetworkTarget::from_str("oylnet").unwrap(),
            NetworkTarget::Regtest
        );
        assert!(NetworkTarget::from_str("unknown").is_err());
    }

    #[test]
    fn passphrase_policy_distinguishes_local_and_public_targets() {
        let mut config = ToolkitConfig::default();
        assert!(config.require_passphrase_policy(&None).is_ok());

        config.network = NetworkTarget::Regtest;
        assert!(config.require_passphrase_policy(&None).is_ok());

        config.network = NetworkTarget::Signet;
        assert!(config.require_passphrase_policy(&None).is_err());
        assert!(config
            .require_passphrase_policy(&Some("secret".to_string()))
            .is_ok());

        config.network = NetworkTarget::Mainnet;
        assert!(config.require_passphrase_policy(&None).is_err());
    }
}
