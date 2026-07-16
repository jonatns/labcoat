//! alkanes-cli wallet discovery and inspection
//!
//! Lists wallets under `~/.alkanes/` and shells out to a locally
//! installed `alkanes-cli` for addresses/balances. Extracted verbatim
//! from the Tauri command layer.

use crate::state::{AddressInfo, AlkanesWallet};

/// List all alkanes-cli wallets in ~/.alkanes/
pub fn list_wallets() -> Result<Vec<AlkanesWallet>, String> {
    let home = std::env::var("HOME").map_err(|e| format!("Failed to get HOME: {}", e))?;
    let wallets_dir = std::path::Path::new(&home).join(".alkanes");

    if !wallets_dir.exists() {
        return Ok(Vec::new());
    }

    let mut wallets = Vec::new();
    let entries =
        std::fs::read_dir(wallets_dir).map_err(|e| format!("Failed to read wallets dir: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        // Only look for .json files
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                // Skip config.json as it's likely not a wallet
                if stem == "config" {
                    continue;
                }

                wallets.push(AlkanesWallet {
                    name: stem.to_string(),
                    file_path: path.to_string_lossy().to_string(),
                    balance: None,
                    addresses: Vec::new(),
                });
            }
        }
    }

    // Sort by name
    wallets.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(wallets)
}

/// Get details for a specific wallet via alkanes-cli.
/// `sync_first` should be true only when bitcoind is reachable.
pub fn wallet_details(wallet_path: &str, sync_first: bool) -> Result<AlkanesWallet, String> {
    let path = std::path::Path::new(wallet_path);
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // 0. SYNC the wallet first (ONLY if bitcoind is running)
    if sync_first {
        // This is required for the balance to be accurate (especially after
        // confirmed txs). Errors are logged but not fatal — we still want to
        // show what we have.
        let sync_res = std::process::Command::new("alkanes-cli")
            .arg("--wallet-file")
            .arg(wallet_path)
            .arg("wallet")
            .arg("sync")
            .output();

        if let Err(e) = sync_res {
            tracing::warn!("Failed to sync wallet {}: {}", name, e);
        }
    } else {
        tracing::info!("Skipping wallet sync for {} (bitcoind not running)", name);
    }

    // 1. Get Addresses (index 0 of each address type)
    let addr_output = std::process::Command::new("alkanes-cli")
        .args(["--wallet-file", wallet_path, "wallet", "addresses"])
        .output()
        .map_err(|e| format!("Failed to run alkanes-cli: {}", e))?;

    let addresses: Vec<AddressInfo> = if addr_output.status.success() {
        parse_addresses(&String::from_utf8_lossy(&addr_output.stdout))
    } else {
        Vec::new()
    };

    // 2. Get Balance by parsing UTXOs
    let utxo_output = std::process::Command::new("alkanes-cli")
        .arg("--wallet-file")
        .arg(wallet_path)
        .arg("wallet")
        .arg("utxos")
        .output();

    let balance = match utxo_output {
        Ok(output) if output.status.success() => {
            Some(parse_balance(&String::from_utf8_lossy(&output.stdout)))
        }
        _ => None,
    };

    Ok(AlkanesWallet {
        name,
        file_path: wallet_path.to_string(),
        balance,
        addresses,
    })
}

/// Parse `alkanes-cli wallet addresses` human output into index-0 addresses
/// per address type, taproot first.
fn parse_addresses(out: &str) -> Vec<AddressInfo> {
    let mut result = Vec::new();
    let mut current_section = "Unknown".to_string();

    for line in out.lines() {
        // Detect section headers like "📋 P2SH Addresses:"
        if line.contains("Addresses:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                // find the part before "Addresses:"
                for (i, part) in parts.iter().enumerate() {
                    if part.contains("Addresses:") && i > 0 {
                        current_section = parts[i - 1].to_string();
                        break;
                    }
                }
            }
            continue;
        }

        // Format: "  0. bcrt1xxx... (index: 0)"
        let trimmed = line.trim();
        if let Some(first_part) = trimmed.split('.').next() {
            if first_part.parse::<usize>().is_ok() {
                let parts: Vec<&str> = trimmed.splitn(2, ". ").collect();
                if parts.len() == 2 {
                    if let Some(addr_part) = parts[1].split(" (index:").next() {
                        let address = addr_part.trim().to_string();
                        // Collect only index 0 of each type ("primary" per type).
                        if trimmed.contains("(index: 0)") {
                            result.push(AddressInfo {
                                address,
                                type_label: current_section.clone(),
                                index: 0,
                            });
                        }
                    }
                }
            }
        }
    }

    // PRIORITIZE TAPROOT (P2TR) addresses at the top
    result.sort_by(|a, b| {
        let a_is_tr = a.type_label.contains("P2TR");
        let b_is_tr = b.type_label.contains("P2TR");
        b_is_tr.cmp(&a_is_tr)
    });

    result
}

/// Parse `alkanes-cli wallet utxos` human output into a formatted BTC balance,
/// filtering immature coinbase (regtest maturity is 100 blocks).
fn parse_balance(out: &str) -> String {
    struct CurrentUtxo {
        amount: u64,
        confs: u64,
        is_coinbase: bool,
        seen: bool,
    }
    let mut current = CurrentUtxo {
        amount: 0,
        confs: 0,
        is_coinbase: false,
        seen: false,
    };
    let mut confirmed_sats: u64 = 0;
    let mut pending_sats: u64 = 0;

    let process_utxo = |utxo: &CurrentUtxo, confirmed: &mut u64, pending: &mut u64| {
        if !utxo.seen {
            return;
        }
        if utxo.is_coinbase && utxo.confs < 100 {
            return;
        }
        if utxo.confs > 0 {
            *confirmed += utxo.amount;
        } else {
            *pending += utxo.amount;
        }
    };

    for line in out.lines() {
        // Detect start of new UTXO (or end of previous) by "Outpoint:"
        if line.contains("Outpoint:") {
            process_utxo(&current, &mut confirmed_sats, &mut pending_sats);
            current = CurrentUtxo {
                amount: 0,
                confs: 0,
                is_coinbase: false,
                seen: true,
            };
        }

        // Parse Amount: filter non-digits to handle ANSI codes
        if line.contains("Amount (sats):") {
            if let Some(rest) = line.split("Amount (sats):").nth(1) {
                let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(val) = digits.parse::<u64>() {
                    current.amount = val;
                }
            }
        }
        if line.contains("Confirmations:") {
            if let Some(rest) = line.split("Confirmations:").nth(1) {
                let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(val) = digits.parse::<u64>() {
                    current.confs = val;
                }
            }
        }
        if line.contains("Properties:") && line.to_lowercase().contains("coinbase") {
            current.is_coinbase = true;
        }
    }
    // Process the last UTXO
    process_utxo(&current, &mut confirmed_sats, &mut pending_sats);

    let total_btc = (confirmed_sats as f64 + pending_sats as f64) / 100_000_000.0;
    format!("{:.8} BTC", total_btc)
}
