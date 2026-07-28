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
