//! labcoat.lock — the per-network deployment ledger.

use crate::error::{LabcoatError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const LOCKFILE: &str = "labcoat.lock";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Lockfile {
    pub version: u32,
    /// network -> contract name -> deployment record
    pub networks: BTreeMap<String, BTreeMap<String, Deployment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub alkanes_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_sha256: Option<String>,
    pub txid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<u64>,
    pub status: String,
    pub deployed_at: u64,
}

pub fn load(dir: &Path) -> Lockfile {
    let path = dir.join(LOCKFILE);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Lockfile {
            version: 1,
            networks: BTreeMap::new(),
        })
}

pub fn save(dir: &Path, lockfile: &Lockfile) -> Result<()> {
    let path = dir.join(LOCKFILE);
    std::fs::write(&path, serde_json::to_string_pretty(lockfile).unwrap())
        .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check permissions"))
}

pub fn record(
    dir: &Path,
    network: &str,
    contract: &str,
    deployment: Deployment,
) -> Result<Lockfile> {
    let mut lockfile = load(dir);
    lockfile.version = 1;
    lockfile
        .networks
        .entry(network.to_string())
        .or_default()
        .insert(contract.to_string(), deployment);
    save(dir, &lockfile)?;
    Ok(lockfile)
}

pub fn get(dir: &Path, network: &str, contract: &str) -> Option<Deployment> {
    load(dir)
        .networks
        .get(network)
        .and_then(|n| n.get(contract))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labcoat_target_does_not_fall_back_to_regtest_records() {
        let root =
            std::env::temp_dir().join(format!("labcoat-lockfile-target-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        record(
            &root,
            "regtest",
            "counter",
            Deployment {
                alkanes_id: "2:1".to_string(),
                wasm_sha256: None,
                txid: "00".repeat(32),
                block: None,
                status: "success".to_string(),
                deployed_at: 0,
            },
        )
        .unwrap();

        assert!(get(&root, "regtest", "counter").is_some());
        assert!(get(&root, "labcoat", "counter").is_none());
        std::fs::remove_dir_all(root).ok();
    }
}
