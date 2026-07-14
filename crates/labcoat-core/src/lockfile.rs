//! labcoat.lock — the per-network deployment ledger, replacing the old
//! deployments/manifest.json (a one-shot migrator is included).

use crate::error::{LabcoatError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const LOCKFILE: &str = "labcoat.lock";
pub const LEGACY_MANIFEST: &str = "deployments/manifest.json";

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

/// One-shot migration from deployments/manifest.json. Entries land under
/// the given network (the legacy manifest was network-blind). Returns the
/// number of migrated deployments; the legacy file is left in place.
pub fn migrate_legacy(dir: &Path, network: &str) -> Result<usize> {
    let legacy_path = dir.join(LEGACY_MANIFEST);
    let Ok(raw) = std::fs::read_to_string(&legacy_path) else {
        return Ok(0);
    };
    let legacy: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("cannot parse {}: {}", legacy_path.display(), e),
            "fix or remove the legacy manifest",
        )
    })?;

    let mut migrated = 0;
    let mut lockfile = load(dir);
    lockfile.version = 1;
    if let Some(entries) = legacy.as_object() {
        for (contract, info) in entries {
            let Some(deployment) = info.get("deployment") else {
                continue;
            };
            let Some(alkanes_id) = deployment.get("alkanesId").and_then(|v| v.as_str()) else {
                continue;
            };
            let record = Deployment {
                alkanes_id: alkanes_id.to_string(),
                wasm_sha256: None,
                txid: deployment
                    .get("txId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                block: None,
                status: deployment
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                deployed_at: deployment
                    .get("deployedAt")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            };
            lockfile
                .networks
                .entry(network.to_string())
                .or_default()
                .insert(contract.clone(), record);
            migrated += 1;
        }
    }
    if migrated > 0 {
        save(dir, &lockfile)?;
    }
    Ok(migrated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_manifest() {
        let dir = std::env::temp_dir().join(format!("labcoat-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("deployments")).unwrap();
        std::fs::write(
            dir.join(LEGACY_MANIFEST),
            r#"{"MyToken": {"abi": "/x.abi.json", "deployment": {"status": "success", "txId": "abc", "alkanesId": "2:1", "deployedAt": 123}}}"#,
        )
        .unwrap();

        let n = migrate_legacy(&dir, "regtest").unwrap();
        assert_eq!(n, 1);
        let dep = get(&dir, "regtest", "MyToken").unwrap();
        assert_eq!(dep.alkanes_id, "2:1");
        assert_eq!(dep.status, "success");

        std::fs::remove_dir_all(&dir).ok();
    }
}
