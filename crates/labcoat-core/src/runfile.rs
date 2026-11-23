//! The run file — `deployments/<network>.json`, the record of every
//! deployment step that ran on one network.
//!
//! It is what makes a step run once: `labcoat plan` compares each step's
//! definition with its record instead of re-deriving state from the
//! chain. Commit it for shared networks; the Labcoat Network file is
//! disposable because `labcoat reset` starts a new chain, which the
//! recorded chain identity (block 1's hash) detects.

use crate::error::{LabcoatError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const RUN_DIR: &str = "deployments";
pub const RUN_SCHEMA: &str = "labcoat/deployment-run/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFile {
    pub schema: String,
    pub network: String,
    /// Chain instance identity (block 1's hash) the records belong to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
    pub steps: BTreeMap<String, StepRecord>,
}

impl RunFile {
    pub fn empty(network: &str, chain_id: Option<String>) -> Self {
        Self {
            schema: RUN_SCHEMA.to_string(),
            network: network.to_string(),
            chain_id,
            steps: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordStatus {
    /// A broadcast was attempted and its result was never recorded.
    Started,
    Success,
    Revert,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRecord {
    /// `external` | `contract` | `call` | `setting`
    pub kind: String,
    pub status: RecordStatus,
    /// Hash of the step's symbolic definition when it ran.
    pub definition: String,
    /// The contract's or external's `block:tx` id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Every Alkane the transaction created, the contract first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<String>,
    /// The id of every step this one referenced when it ran.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub refs: BTreeMap<String, String>,
    /// Arguments as executed, with references and `height` resolved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// A setting's value as last written or observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_sha256: Option<String>,
    /// File stem of the contract's build artifact under `build/`. A
    /// step's name need not match its package's, so tools resolving the
    /// local build go through this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    /// `deployed` | `cloned` | `adopted` | `imported`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Chain height the step's values were evaluated at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
    pub updated_at: u64,
    /// Earlier instances this record replaced, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<StepRecord>,
}

impl StepRecord {
    pub fn new(kind: &str, status: RecordStatus, definition: String) -> Self {
        Self {
            kind: kind.to_string(),
            status,
            definition,
            id: None,
            created: Vec::new(),
            refs: BTreeMap::new(),
            args: Vec::new(),
            value: None,
            txid: None,
            wasm_sha256: None,
            artifact: None,
            origin: None,
            height: None,
            updated_at: now_millis(),
            history: Vec::new(),
        }
    }

    /// Carry the instance this record replaces, and its history, forward.
    pub fn replacing(mut self, previous: Option<&StepRecord>) -> Self {
        if let Some(previous) = previous {
            let mut history = previous.history.clone();
            if previous.status == RecordStatus::Success {
                let mut earlier = previous.clone();
                earlier.history.clear();
                history.push(earlier);
            }
            self.history = history;
        }
        self
    }
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn path(root: &Path, network: &str) -> PathBuf {
    root.join(RUN_DIR).join(format!("{network}.json"))
}

/// Load a network's run file. A missing file is an empty run; an
/// unreadable or corrupt file is an error — silently starting empty would
/// run every step a second time.
pub fn load(root: &Path, network: &str) -> Result<RunFile> {
    let path = path(root, network);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RunFile::empty(network, None))
        }
        Err(e) => {
            return Err(LabcoatError::new(
                "RUN_FILE_INVALID",
                format!("cannot read {}: {e}", path.display()),
                "check permissions on the deployments directory",
            ))
        }
    };
    let run: RunFile = serde_json::from_str(&text).map_err(|e| {
        LabcoatError::new(
            "RUN_FILE_INVALID",
            format!("{} is corrupt: {e}", path.display()),
            "restore the file from version control, or repair the JSON by hand",
        )
    })?;
    if run.schema != RUN_SCHEMA {
        return Err(LabcoatError::new(
            "RUN_FILE_INVALID",
            format!(
                "{} has schema `{}`, but this labcoat reads `{RUN_SCHEMA}`",
                path.display(),
                run.schema
            ),
            "use the labcoat release that wrote the file",
        ));
    }
    Ok(run)
}

/// Atomically replace a network's run file.
pub fn save(root: &Path, run: &RunFile) -> Result<()> {
    let path = path(root, &run.network);
    let dir = path.parent().expect("run file path has a parent");
    let io_err = |e: std::io::Error| {
        LabcoatError::new(
            "RUN_FILE_INVALID",
            format!("cannot write {}: {e}", path.display()),
            "check permissions on the deployments directory",
        )
    };
    std::fs::create_dir_all(dir).map_err(io_err)?;
    let tmp = dir.join(format!(".{}.json.tmp-{}", run.network, std::process::id()));
    let write = (|| {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        let mut text = serde_json::to_string_pretty(run).expect("serializable run file");
        text.push('\n');
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    write.map_err(io_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("labcoat-run-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn round_trips_and_rejects_corruption() {
        let root = temp_root("roundtrip");
        assert!(load(&root, "labcoat").unwrap().steps.is_empty());

        let mut run = RunFile::empty("labcoat", Some("abc".into()));
        let mut record = StepRecord::new("contract", RecordStatus::Success, "def".into());
        record.id = Some("2:5".into());
        run.steps.insert("token".into(), record);
        save(&root, &run).unwrap();

        let loaded = load(&root, "labcoat").unwrap();
        assert_eq!(loaded.chain_id.as_deref(), Some("abc"));
        assert_eq!(loaded.steps["token"].id.as_deref(), Some("2:5"));

        std::fs::write(path(&root, "labcoat"), "{").unwrap();
        assert_eq!(load(&root, "labcoat").unwrap_err().code, "RUN_FILE_INVALID");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_replacement_keeps_earlier_instances_oldest_first() {
        let mut first = StepRecord::new("contract", RecordStatus::Success, "a".into());
        first.id = Some("2:5".into());
        let mut second =
            StepRecord::new("contract", RecordStatus::Success, "b".into()).replacing(Some(&first));
        second.id = Some("2:9".into());
        let third =
            StepRecord::new("contract", RecordStatus::Success, "c".into()).replacing(Some(&second));

        let ids: Vec<_> = third
            .history
            .iter()
            .map(|record| record.id.clone().unwrap())
            .collect();
        assert_eq!(ids, ["2:5", "2:9"]);
        assert!(third.history.iter().all(|record| record.history.is_empty()));
    }
}
