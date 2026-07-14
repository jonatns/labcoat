//! Wallet lifecycle over the alkanes keystore (BIP-86/84/49/44 — the same
//! derivation paths the old oyl-sdk `Signer` used, so a given mnemonic
//! yields the same addresses as before the rebase).

use crate::error::{LabcoatError, Result};
use crate::system::ToolkitConfig;
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::{WalletConfig, WalletProvider};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletInitResult {
    pub address: String,
    pub network: String,
    pub wallet_file: String,
    pub created: bool,
    /// Present only when a mnemonic was generated (not supplied) — the one
    /// chance to write it down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnemonic: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletAddress {
    pub address: String,
    pub script_type: String,
    pub derivation_path: String,
    pub index: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletUtxo {
    pub txid: String,
    pub vout: u32,
    pub amount: u64,
    pub address: String,
    pub confirmations: u32,
    pub frozen: bool,
    pub has_inscriptions: bool,
    pub has_runes: bool,
    pub has_alkanes: bool,
    pub is_coinbase: bool,
}

fn wallet_config(config: &ToolkitConfig) -> WalletConfig {
    let network = match config.normalized_network().as_str() {
        "mainnet" => bitcoin::Network::Bitcoin,
        "testnet" => bitcoin::Network::Testnet,
        "signet" => bitcoin::Network::Signet,
        _ => bitcoin::Network::Regtest,
    };
    WalletConfig {
        wallet_path: config.wallet_file.display().to_string(),
        network,
        bitcoin_rpc_url: config.jsonrpc_url.clone(),
        metashrew_rpc_url: config.jsonrpc_url.clone(),
        network_params: None,
    }
}

/// Create (or load, if it already exists) the project wallet.
/// The mnemonic never crosses on argv — callers read it from stdin/env.
pub async fn init(
    provider: &mut ConcreteProvider,
    config: &ToolkitConfig,
    mnemonic: Option<String>,
    passphrase: Option<String>,
) -> Result<WalletInitResult> {
    let exists = config.wallet_file.exists();
    let generated = mnemonic.is_none() && !exists;

    if exists {
        let info = provider
            .load_wallet(wallet_config(config), passphrase)
            .await
            .map_err(|e| LabcoatError::classify(e.into()))?;
        return Ok(WalletInitResult {
            address: info.address,
            network: config.normalized_network(),
            wallet_file: config.wallet_file.display().to_string(),
            created: false,
            mnemonic: None,
        });
    }

    let info = provider
        .create_wallet(wallet_config(config), mnemonic, passphrase)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;

    // Upstream create_wallet writes `<stem>-<timestamp>.json` (never the
    // configured path, to avoid clobbering). Labcoat wants the keystore at
    // the exact configured path — move the newest timestamped sibling there.
    if !config.wallet_file.exists() {
        let parent = config
            .wallet_file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let stem = config
            .wallet_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("wallet");
        let mut candidates: Vec<std::path::PathBuf> = std::fs::read_dir(&parent)
            .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check permissions"))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with(&format!("{}-", stem)) && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();
        candidates.sort();
        if let Some(newest) = candidates.pop() {
            std::fs::rename(&newest, &config.wallet_file).map_err(|e| {
                LabcoatError::new(
                    "TOOLKIT_ERROR",
                    format!("could not move keystore into place: {}", e),
                    "check permissions",
                )
            })?;
        }
    }

    Ok(WalletInitResult {
        address: info.address,
        network: config.normalized_network(),
        wallet_file: config.wallet_file.display().to_string(),
        created: true,
        mnemonic: if generated { info.mnemonic } else { None },
    })
}

/// First `count` receive addresses per script type.
pub async fn addresses(provider: &ConcreteProvider, count: u32) -> Result<Vec<WalletAddress>> {
    let addrs = WalletProvider::get_addresses(provider, count)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    Ok(addrs
        .into_iter()
        .map(|a| WalletAddress {
            address: a.address,
            script_type: a.script_type,
            derivation_path: a.derivation_path,
            index: a.index,
        })
        .collect())
}

/// The wallet's primary receive address (taproot preferred).
pub async fn primary_address(provider: &ConcreteProvider) -> Result<String> {
    let addrs = addresses(provider, 1).await?;
    addrs
        .iter()
        .find(|a| a.script_type.to_lowercase().contains("p2tr"))
        .or_else(|| addrs.first())
        .map(|a| a.address.clone())
        .ok_or_else(|| {
            LabcoatError::new(
                "WALLET_MISSING",
                "wallet has no derivable addresses",
                "run `labcoat wallet init`",
            )
        })
}

/// Spendable UTXOs for the wallet.
pub async fn utxos(provider: &ConcreteProvider) -> Result<Vec<WalletUtxo>> {
    let utxos = provider
        .get_utxos(false, None)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    Ok(utxos
        .into_iter()
        .map(|(outpoint, info)| WalletUtxo {
            txid: outpoint.txid.to_string(),
            vout: outpoint.vout,
            amount: info.amount,
            address: info.address,
            confirmations: info.confirmations,
            frozen: info.frozen,
            has_inscriptions: info.has_inscriptions,
            has_runes: info.has_runes,
            has_alkanes: info.has_alkanes,
            is_coinbase: info.is_coinbase,
        })
        .collect())
}
