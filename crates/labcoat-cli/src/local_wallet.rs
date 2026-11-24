//! The wallet Labcoat Network comes with.
//!
//! Keys belong to the machine, not to a project: the chain is shared by
//! every project on it, and a keystore inside a repository is one mistaken
//! commit, archive, or upload away from leaking. The built-in wallet is a
//! random key under a fixed development passphrase, so it must never sign
//! on another network; `settings` refuses that.

use isomer_core::LabcoatNetwork;
use labcoat_core::{system, wallet, LabcoatError, NetworkTarget, ToolkitConfig};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Fixed on purpose: the built-in wallet only ever holds worthless coins,
/// and a passphrase meant for another wallet must not end up protecting it.
pub const PASSPHRASE: &str = "labcoat-dev";

/// Below this, `up` tops the wallet up, and `up --ci` reports failure.
pub const MINIMUM_SATS: u64 = 100_000_000;
const FUNDING_BTC: f64 = 10.0;

/// The default keystore for `network`, outside any project.
pub fn default_path(network: NetworkTarget) -> PathBuf {
    isomer_core::config::get_wallets_dir().join(format!("{}.json", network.id()))
}

/// Whether `path` is the built-in Labcoat Network wallet.
pub fn is_built_in(path: &std::path::Path) -> bool {
    let built_in = default_path(NetworkTarget::Labcoat);
    path == built_in
        || match (path.canonicalize(), built_in.canonicalize()) {
            (Ok(path), Ok(built_in)) => path == built_in,
            _ => false,
        }
}

async fn balance(config: &ToolkitConfig) -> Result<u64, LabcoatError> {
    let provider = system::connect(config, Some(PASSPHRASE.into()), true).await?;
    Ok(wallet::utxos(&provider)
        .await?
        .iter()
        .map(|utxo| utxo.amount)
        .sum())
}

fn network_error(message: String) -> LabcoatError {
    LabcoatError::new("LABCOAT_NETWORK_ERROR", message, "check `labcoat status`")
}

/// Create the built-in wallet if needed and top it up from the faucet.
/// The network must be running.
async fn ensure(network: &LabcoatNetwork) -> Result<Value, LabcoatError> {
    let config = ToolkitConfig {
        network: NetworkTarget::Labcoat,
        // The network this handle controls, which need not be on the
        // default port.
        rpc_url: isomer_core::rpc::rpc_url(&network.config),
        wallet_file: default_path(NetworkTarget::Labcoat),
        ..ToolkitConfig::default()
    };
    let created = !config.wallet_file.exists();
    if created {
        // The mnemonic is dropped: nothing worth recovering may live here.
        let mut provider = system::connect(&config, Some(PASSPHRASE.into()), false).await?;
        wallet::init(&mut provider, &config, None, Some(PASSPHRASE.into())).await?;
    }
    let provider = system::connect(&config, Some(PASSPHRASE.into()), true).await?;
    let address = wallet::primary_address(&provider).await?;
    let mut sats = balance(&config).await?;
    if sats < MINIMUM_SATS {
        network
            .fund(&address, FUNDING_BTC)
            .await
            .map_err(network_error)?;
        network.mine(1, None).await.map_err(network_error)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        while sats < MINIMUM_SATS && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
            sats = balance(&config).await?;
        }
        if sats < MINIMUM_SATS {
            return Err(network_error(
                "the faucet transaction was mined but is not indexed yet".into(),
            ));
        }
    }
    Ok(json!({
        "address": address,
        "balanceSats": sats,
        "created": created,
        "walletFile": config.wallet_file.display().to_string(),
    }))
}

/// What `up` reports. A wallet problem never takes the network down with
/// it, so it is reported instead of returned.
pub async fn prepare(network: &mut LabcoatNetwork) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !network.status().await.is_ready {
        if Instant::now() > deadline {
            return json!({ "error": "Labcoat Network was not ready in time" });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    match ensure(network).await {
        Ok(wallet) => wallet,
        Err(error) => json!({ "error": error.message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_network_has_its_own_keystore_and_only_labcoats_is_built_in() {
        let local = default_path(NetworkTarget::Labcoat);
        let signet = default_path(NetworkTarget::Signet);
        assert!(local.ends_with("wallets/labcoat.json"));
        assert!(signet.ends_with("wallets/signet.json"));
        assert!(is_built_in(&local));
        assert!(!is_built_in(&signet));
    }
}
