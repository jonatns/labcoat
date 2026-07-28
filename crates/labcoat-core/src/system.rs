//! Provider bootstrap against the pinned alkanes-rs main commit.
//!
//! Mirrors what alkanes-cli's `SystemAlkanes::new_with_options` does for
//! our fixed shape (Qubitcoin RPC endpoint, project-local keystore),
//! without depending on `alkanes-cli-sys` (broken at the pinned rev) or
//! clap `Args`.

use crate::error::{LabcoatError, Result};
use alkanes_cli_common::provider::ConcreteProvider;
use std::path::PathBuf;

/// Connection + wallet settings for the toolkit.
#[derive(Debug, Clone)]
pub struct ToolkitConfig {
    /// Network name: regtest | signet | mainnet | testnet.
    /// ("oylnet" is accepted as a deprecated alias for regtest.)
    pub network: String,
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
            network: "regtest".to_string(),
            rpc_url: "http://127.0.0.1:18443".to_string(),
            wallet_file: PathBuf::from(".labcoat/wallet.json"),
            fee_rate: Some(2.0),
        }
    }
}

impl ToolkitConfig {
    /// Normalize deprecated network aliases.
    pub fn normalized_network(&self) -> String {
        if self.network == "oylnet" {
            tracing::warn!("network 'oylnet' is deprecated; treating as 'regtest'");
            "regtest".to_string()
        } else {
            self.network.clone()
        }
    }

    /// Refuse footgun setups: mainnet/signet require an explicit passphrase.
    pub fn require_passphrase_policy(&self, passphrase: &Option<String>) -> Result<()> {
        let net = self.normalized_network();
        if passphrase.is_none() && (net == "mainnet" || net == "signet") {
            return Err(LabcoatError::new(
                "WALLET_LOCKED",
                format!("a wallet passphrase is required on {}", net),
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
    let network = config.normalized_network();

    // Network params + process-global network (address derivation/signing).
    let params =
        alkanes_cli_common::network::NetworkParams::from_network_str(&network).map_err(|e| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!("unknown network '{}': {}", network, e),
                "use one of: regtest, signet, testnet, mainnet",
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
        network,
        Some(config.wallet_file.clone()),
        Vec::new(),
    )
    .await
    .map_err(|e| LabcoatError::classify(e.into()))?;
    provider.rpc_config.qubitcoin_rpc_url = Some(config.rpc_url.clone());

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

    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn provider_uses_qubitcoin_mode_without_gateway() {
        let config = ToolkitConfig::default();
        let provider = connect(&config, None, false).await.unwrap();
        assert_eq!(
            provider.rpc_config.qubitcoin_rpc_url.as_deref(),
            Some("http://127.0.0.1:18443")
        );
        assert!(provider.rpc_config.jsonrpc_url.is_none());
        assert!(provider.rpc_config.is_qubitcoin_mode());
    }

    #[test]
    fn toolkit_defaults_to_local_qubitcoin_rpc() {
        assert_eq!(ToolkitConfig::default().rpc_url, "http://127.0.0.1:18443");
    }
}
