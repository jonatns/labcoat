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
    /// Chain instance identity: the hash of block 1 at deploy time. A
    /// `labcoat reset` produces a different block 1, which marks every
    /// record from the previous chain as stale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
}

/// True when the record was written against a different chain instance
/// (e.g. before a `labcoat reset`). Records without a chain id cannot be
/// distinguished and are treated as current.
pub fn is_stale(deployment: &Deployment, current_chain_id: &str) -> bool {
    deployment
        .chain_id
        .as_deref()
        .is_some_and(|recorded| recorded != current_chain_id)
}

/// Load the lockfile. A missing file is an empty ledger; an unreadable or
/// corrupt file is an error — silently starting empty would overwrite the
/// deployment history on the next save.
pub fn load(dir: &Path) -> Result<Lockfile> {
    let path = dir.join(LOCKFILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Lockfile {
                version: 1,
                networks: BTreeMap::new(),
            })
        }
        Err(e) => {
            return Err(LabcoatError::new(
                "LOCKFILE_INVALID",
                format!("cannot read {}: {}", path.display(), e),
                "check permissions on labcoat.lock",
            ))
        }
    };
    serde_json::from_str(&text).map_err(|e| {
        LabcoatError::new(
            "LOCKFILE_INVALID",
            format!("{} is corrupt: {}", path.display(), e),
            "repair the JSON by hand, or delete labcoat.lock to start a fresh ledger (deployment records will be lost)",
        )
    })
}

/// Atomically replace the lockfile (temp file + rename in the same
/// directory), so a crash mid-write never leaves a truncated ledger.
pub fn save(dir: &Path, lockfile: &Lockfile) -> Result<()> {
    let path = dir.join(LOCKFILE);
    let tmp = dir.join(format!(".{}.tmp-{}", LOCKFILE, std::process::id()));
    let io_err =
        |e: std::io::Error| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check permissions");
    let write = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        use std::io::Write;
        file.write_all(serde_json::to_string_pretty(lockfile).unwrap().as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write.map_err(io_err)
}

pub fn record(
    dir: &Path,
    network: &str,
    contract: &str,
    deployment: Deployment,
) -> Result<Lockfile> {
    let mut lockfile = load(dir)?;
    lockfile.version = 1;
    lockfile
        .networks
        .entry(network.to_string())
        .or_default()
        .insert(contract.to_string(), deployment);
    save(dir, &lockfile)?;
    Ok(lockfile)
}

pub fn get(dir: &Path, network: &str, contract: &str) -> Result<Option<Deployment>> {
    Ok(load(dir)?
        .networks
        .get(network)
        .and_then(|n| n.get(contract))
        .cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deployment(chain_id: Option<&str>) -> Deployment {
        Deployment {
            alkanes_id: "2:1".to_string(),
            wasm_sha256: None,
            txid: "00".repeat(32),
            block: None,
            status: "success".to_string(),
            deployed_at: 0,
            chain_id: chain_id.map(String::from),
        }
    }

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("labcoat-lockfile-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn labcoat_target_does_not_fall_back_to_regtest_records() {
        let root = temp_root("target");
        record(&root, "regtest", "counter", deployment(None)).unwrap();

        assert!(get(&root, "regtest", "counter").unwrap().is_some());
        assert!(get(&root, "labcoat", "counter").unwrap().is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn corrupt_lockfiles_are_errors_not_empty_ledgers() {
        let root = temp_root("corrupt");
        std::fs::write(root.join(LOCKFILE), "{ not json").unwrap();

        assert_eq!(load(&root).unwrap_err().code, "LOCKFILE_INVALID");
        assert_eq!(
            get(&root, "labcoat", "counter").unwrap_err().code,
            "LOCKFILE_INVALID"
        );
        // A corrupt ledger must never be silently replaced by a record.
        assert_eq!(
            record(&root, "labcoat", "counter", deployment(None))
                .unwrap_err()
                .code,
            "LOCKFILE_INVALID"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn missing_lockfiles_load_as_empty_ledgers() {
        let root = temp_root("missing");
        assert!(load(&root).unwrap().networks.is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn save_leaves_no_temp_files_and_round_trips() {
        let root = temp_root("atomic");
        record(&root, "labcoat", "counter", deployment(Some("abc"))).unwrap();
        record(&root, "labcoat", "token", deployment(Some("abc"))).unwrap();

        let names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec![LOCKFILE.to_string()]);

        let loaded = load(&root).unwrap();
        assert_eq!(loaded.networks["labcoat"].len(), 2);
        assert_eq!(
            loaded.networks["labcoat"]["counter"].chain_id.as_deref(),
            Some("abc")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn pre_chain_id_records_still_load_and_are_never_stale() {
        let root = temp_root("compat");
        std::fs::write(
            root.join(LOCKFILE),
            r#"{"version":1,"networks":{"labcoat":{"counter":{"alkanesId":"2:1","txid":"00","status":"success","deployedAt":0}}}}"#,
        )
        .unwrap();

        let dep = get(&root, "labcoat", "counter").unwrap().unwrap();
        assert_eq!(dep.chain_id, None);
        assert!(!is_stale(&dep, "anything"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn records_from_a_reset_chain_are_stale() {
        let dep = deployment(Some("block-one-hash-a"));
        assert!(!is_stale(&dep, "block-one-hash-a"));
        assert!(is_stale(&dep, "block-one-hash-b"));
    }
}
