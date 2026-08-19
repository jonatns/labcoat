//! Version-2 durable deployment state.
//!
//! Three files coexist under a project:
//!
//! - `labcoat.lock` — the v1 per-network ledger (`lockfile.rs`). Still the
//!   active-address book and the canonical source for name resolution; the
//!   compatibility export regenerates it from this module's state.
//! - `.labcoat/state/<network>.json` — the apply call journal
//!   (`apply.rs`), untouched by this module.
//! - `.labcoat/state/<environment>/state.json` — this schema: per
//!   environment, with a lineage UUID, a monotonic serial, chain identity,
//!   and append-only instance history per resource. The surrounding
//!   directory also holds the environment lease (`state.lock`) and
//!   `backups/`.
//!
//! Persistence, locking, and the atomic commit protocol live in
//! `state_backend.rs`; this module owns the schema, validation, the v1
//! migration, and the compatibility export.

use crate::error::{LabcoatError, Result};
use crate::lockfile;
use crate::state_backend::{self, StateLease};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const STATE_VERSION: u32 = 2;
pub const STATE_FILE: &str = "state.json";
pub const LOCK_FILE: &str = "state.lock";
pub const BACKUPS_DIR: &str = "backups";

/// Version-2 operational state for one environment.
///
/// `deny_unknown_fields` throughout is deliberate fail-closed policy:
/// state written by a future schema must not be silently half-read.
/// Additive changes bump `version`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    /// Persisted schema version; anything but 2 fails closed.
    pub version: u32,
    /// UUID that survives serial increments and changes only when state is
    /// intentionally re-created.
    pub lineage: String,
    /// Monotonic revision; incremented exactly once per successful commit.
    pub serial: u64,
    pub environment: String,
    pub chain: ChainIdentity,
    /// Logical resource address ("contract.<name>") -> resource record.
    pub resources: BTreeMap<String, Resource>,
    /// Resumable apply journal. Schema-only in Milestone 1: always
    /// present, written empty; the apply engine (later milestones) owns
    /// appending to it.
    pub operations: Vec<Operation>,
}

/// The chain instance this state belongs to. Network name alone cannot
/// distinguish two regtest instances or detect a reset, so identity is the
/// hash of block 1 (regtest block 0 is identical across resets) plus, for
/// the managed Labcoat Network, a persistent instance UUID that
/// `labcoat reset` regenerates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChainIdentity {
    pub network: String,
    pub bitcoin_network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block1_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labcoat_network_instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resource {
    pub kind: ResourceKind,
    /// The instance the compatibility export publishes; None when empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_instance: Option<String>,
    /// Append-only, oldest first. Replacements append and move
    /// `active_instance`; nothing is ever erased.
    pub instances: Vec<Instance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    Contract,
}

/// One on-chain instance of a resource. Field semantics match the v1
/// `lockfile::Deployment` record exactly so the compatibility export can
/// reproduce it byte-for-byte; the extra fields are populated only by
/// deploy-time recording, never invented by migration.
///
/// A per-instance lifecycle status is intentionally absent in Milestone 1:
/// "superseded" is derivable (`instance_id != resource.active_instance`),
/// and lifecycle enums arrive with the planner under a version bump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Instance {
    /// "instance-<n>", assigned in append order, unique within a resource.
    pub instance_id: String,
    pub origin: InstanceOrigin,
    pub alkanes_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_sha256: Option<String>,
    /// Reveal txid — including the literal "adopted" sentinel written for
    /// reserve adoptions — kept verbatim from v1.
    pub txid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_txid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<u64>,
    /// "success" | "revert" | "unknown" — the v1 execution status string.
    pub status: String,
    /// Unix milliseconds (v1 `deployedAt` semantics).
    pub deployed_at: u64,
    /// Block-1 hash at creation time; may differ from
    /// `State::chain::block1_hash` for imported records written before a
    /// reset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labcoat_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceOrigin {
    /// Recorded live by a labcoat deploy.
    Deployed,
    /// Imported from a v1 labcoat.lock record by `state migrate`.
    Imported,
    /// Recorded by an apply reserve adoption.
    Adopted,
}

/// A journaled apply operation. Milestone 1 persists the schema only —
/// `operations` is always written empty; the transitions below are the
/// durable checkpoints the apply engine will persist between irreversible
/// steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Operation {
    pub operation_id: String,
    pub resource: String,
    pub action: OperationAction,
    pub status: OperationStatus,
    pub transitions: Vec<OperationTransition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationAction {
    Create,
    Import,
    Adopt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationStatus {
    Prepared,
    CommitBroadcast,
    CommitConfirmed,
    RevealBroadcast,
    RevealConfirmed,
    Indexed,
    Verified,
    Reverted,
    Failed,
    Orphaned,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationTransition {
    pub status: OperationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<String>,
    /// Unix milliseconds.
    pub at: u64,
}

/// A random RFC 4122 version-4 UUID for state lineage, without pulling in
/// the `uuid` crate.
pub fn new_lineage() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    format!(
        "{}-{}-{}-{}-{}",
        hex::encode(&b[0..4]),
        hex::encode(&b[4..6]),
        hex::encode(&b[6..8]),
        hex::encode(&b[8..10]),
        hex::encode(&b[10..16])
    )
}

pub fn new_state(environment: &str, chain: ChainIdentity, lineage: String) -> State {
    State {
        version: STATE_VERSION,
        lineage,
        serial: 0,
        environment: environment.to_string(),
        chain,
        resources: BTreeMap::new(),
        operations: Vec::new(),
    }
}

/// The single formatting authority for persisted state — the backend and
/// the golden tests must agree on these exact bytes.
pub fn to_json_string(state: &State) -> String {
    let mut text = serde_json::to_string_pretty(state).expect("state serializes");
    text.push('\n');
    text
}

/// Parse and validate persisted state. The version probe runs before the
/// strict parse so a future schema reports `STATE_UNSUPPORTED` rather than
/// an unknown-field `STATE_INVALID`.
pub fn parse(text: &str, environment: &str) -> Result<State> {
    let probe: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        LabcoatError::new(
            "STATE_INVALID",
            format!("durable state is corrupt: {e}"),
            "restore backups/state.json.prev, or archive the environment directory and re-run `labcoat state migrate`",
        )
    })?;
    match probe.get("version").and_then(serde_json::Value::as_u64) {
        Some(v) if v == u64::from(STATE_VERSION) => {}
        Some(v) => {
            return Err(LabcoatError::new(
                "STATE_UNSUPPORTED",
                format!("durable state schema version {v} is not supported by this labcoat (expected {STATE_VERSION})"),
                "upgrade labcoat, or restore a backup from .labcoat/state/<environment>/backups",
            ))
        }
        None => {
            return Err(LabcoatError::new(
                "STATE_INVALID",
                "durable state has no schema version".to_string(),
                "restore backups/state.json.prev, or archive the environment directory and re-run `labcoat state migrate`",
            ))
        }
    }
    let state: State = serde_json::from_str(text).map_err(|e| {
        LabcoatError::new(
            "STATE_INVALID",
            format!("durable state is corrupt: {e}"),
            "restore backups/state.json.prev, or archive the environment directory and re-run `labcoat state migrate`",
        )
    })?;
    if state.environment != environment {
        return Err(LabcoatError::new(
            "STATE_INVALID",
            format!(
                "durable state belongs to environment '{}' but lives in the '{}' directory",
                state.environment, environment
            ),
            "the state file was moved by hand; restore it to its own environment directory",
        ));
    }
    Ok(state)
}

/// Environment names become path components under `.labcoat/state/`, so
/// they are restricted the same way snapshot names are.
pub fn validate_environment_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!("invalid environment name '{name}'"),
            "environment names must be non-empty [a-zA-Z0-9_-]",
        ));
    }
    Ok(())
}

/// Reject mutation when the observed chain is not the one this state was
/// written against. Recorded-None fields cannot be compared and stay
/// backfillable; observed-None against a recorded value fails closed —
/// "cannot verify" must never become "assumed unchanged".
pub fn validate_chain(recorded: &ChainIdentity, observed: &ChainIdentity) -> Result<()> {
    if recorded.network != observed.network || recorded.bitcoin_network != observed.bitcoin_network
    {
        return Err(LabcoatError::new(
            "STATE_CHAIN_MISMATCH",
            format!(
                "durable state was written for network '{}' ({}) but the current target is '{}' ({})",
                recorded.network,
                recorded.bitcoin_network,
                observed.network,
                observed.bitcoin_network
            ),
            "each environment binds to one network; use a different --environment",
        ));
    }
    let mismatch = |what: &str, recorded_value: &str, observed_value: Option<&str>| {
        LabcoatError::new(
            "STATE_CHAIN_MISMATCH",
            match observed_value {
                Some(observed_value) => format!(
                    "the chain was reset since this state was written: recorded {what} {recorded_value}, observed {observed_value}"
                ),
                None => format!(
                    "cannot verify the chain identity: state records {what} {recorded_value} but none is observable now"
                ),
            },
            "archive .labcoat/state/<environment> or use a different --environment for the new chain",
        )
    };
    if let Some(recorded_id) = recorded.labcoat_network_instance_id.as_deref() {
        match observed.labcoat_network_instance_id.as_deref() {
            Some(observed_id) if observed_id == recorded_id => {}
            other => return Err(mismatch("instance id", recorded_id, other)),
        }
    }
    if let Some(recorded_hash) = recorded.block1_hash.as_deref() {
        match observed.block1_hash.as_deref() {
            Some(observed_hash) if observed_hash == recorded_hash => {}
            other => return Err(mismatch("block-1 hash", recorded_hash, other)),
        }
    }
    Ok(())
}

impl Instance {
    /// Build an instance from a v1 record, copying every v1 field verbatim
    /// and inventing nothing.
    pub fn from_v1(
        instance_id: String,
        origin: InstanceOrigin,
        deployment: &lockfile::Deployment,
    ) -> Instance {
        Instance {
            instance_id,
            origin,
            alkanes_id: deployment.alkanes_id.clone(),
            wasm_sha256: deployment.wasm_sha256.clone(),
            txid: deployment.txid.clone(),
            commit_txid: None,
            block: deployment.block,
            status: deployment.status.clone(),
            deployed_at: deployment.deployed_at,
            chain_id: deployment.chain_id.clone(),
            labcoat_version: None,
            revert_reason: None,
        }
    }

    /// Reconstruct the exact v1 record (dropping the fields v1 never had).
    pub fn to_v1(&self) -> lockfile::Deployment {
        lockfile::Deployment {
            alkanes_id: self.alkanes_id.clone(),
            wasm_sha256: self.wasm_sha256.clone(),
            txid: self.txid.clone(),
            block: self.block,
            status: self.status.clone(),
            deployed_at: self.deployed_at,
            chain_id: self.chain_id.clone(),
        }
    }
}

/// The logical resource address for a lockfile contract name.
pub fn resource_address(contract: &str) -> String {
    format!("contract.{contract}")
}

pub struct MigrationInputs<'a> {
    pub lockfile: &'a lockfile::Lockfile,
    /// Records of this network become resources; other networks' records
    /// stay in labcoat.lock untouched (one state file = one chain).
    pub network: &'a str,
    pub environment: &'a str,
    pub chain: ChainIdentity,
    /// Injected so tests can produce deterministic golden output.
    pub lineage: String,
}

/// Pure v1 -> v2 conversion: one `imported` instance per record of the
/// selected network. `active_instance` is set regardless of v1 status —
/// revert records are part of the address book and must round-trip.
/// Serial stays 0; the backend commit makes it 1.
pub fn migrate_v1(inputs: &MigrationInputs<'_>) -> State {
    let mut state = new_state(
        inputs.environment,
        inputs.chain.clone(),
        inputs.lineage.clone(),
    );
    if let Some(records) = inputs.lockfile.networks.get(inputs.network) {
        for (name, deployment) in records {
            let instance = Instance::from_v1(
                "instance-1".to_string(),
                InstanceOrigin::Imported,
                deployment,
            );
            state.resources.insert(
                resource_address(name),
                Resource {
                    kind: ResourceKind::Contract,
                    active_instance: Some(instance.instance_id.clone()),
                    instances: vec![instance],
                },
            );
        }
    }
    state
}

/// Regenerate labcoat.lock as the active-address book: replace only the
/// state's own network subtree with the active instances' v1 records,
/// preserve every other network's subtree untouched, and write through
/// `lockfile::save` (same atomic writer, same formatting). Returns whether
/// the file was written.
///
/// Round-trip guarantee: for a lockfile previously written by labcoat,
/// migrate -> export reproduces the file byte-identically.
pub fn export_lockfile(root: &Path, state: &State) -> Result<bool> {
    let mut lock = lockfile::load(root)?;
    lock.version = 1;
    let mut records = BTreeMap::new();
    for (address, resource) in &state.resources {
        let Some(active_id) = resource.active_instance.as_deref() else {
            continue;
        };
        let Some(instance) = resource
            .instances
            .iter()
            .find(|i| i.instance_id == active_id)
        else {
            continue;
        };
        let name = address
            .strip_prefix("contract.")
            .unwrap_or(address)
            .to_string();
        records.insert(name, instance.to_v1());
    }
    if records.is_empty() {
        lock.networks.remove(&state.chain.network);
    } else {
        lock.networks.insert(state.chain.network.clone(), records);
    }
    // Don't conjure an empty labcoat.lock where none existed.
    if !root.join(lockfile::LOCKFILE).exists() && lock.networks.is_empty() {
        return Ok(false);
    }
    lockfile::save(root, &lock)?;
    Ok(true)
}

#[derive(Debug)]
pub struct MigrateOutcome {
    pub state_path: PathBuf,
    pub backup: Option<PathBuf>,
    pub resources: usize,
    pub instances: usize,
    pub lockfile_regenerated: bool,
    pub serial: u64,
    pub lineage: String,
}

/// Create version-2 durable state from the v1 ledger. Non-destructive by
/// construction: refuses to run onto existing state, takes a timestamped
/// backup of labcoat.lock before its only side effects, and the final
/// lockfile rewrite is content-identical.
pub fn migrate(
    root: &Path,
    environment: &str,
    network: &str,
    chain: ChainIdentity,
    lineage: String,
    now_millis: u64,
) -> Result<MigrateOutcome> {
    let mut lease = StateLease::acquire(root, environment)?;
    if lease.load()?.is_some() {
        return Err(LabcoatError::new(
            "STATE_CONFLICT",
            format!("version 2 durable state already exists for environment '{environment}'"),
            "state migrate runs once per environment; inspect it with `labcoat state list`",
        ));
    }
    let lock = lockfile::load(root)?;
    let lock_path = root.join(lockfile::LOCKFILE);
    let backup = if lock_path.exists() {
        Some(lease.backup_file(&lock_path, lockfile::LOCKFILE, now_millis)?)
    } else {
        None
    };
    let state = migrate_v1(&MigrationInputs {
        lockfile: &lock,
        network,
        environment,
        chain,
        lineage,
    });
    let state = lease.commit(0, state)?;
    let lockfile_regenerated = export_lockfile(root, &state)?;
    Ok(MigrateOutcome {
        state_path: state_backend::state_path(root, environment)?,
        backup,
        resources: state.resources.len(),
        instances: state.resources.values().map(|r| r.instances.len()).sum(),
        lockfile_regenerated,
        serial: state.serial,
        lineage: state.lineage,
    })
}

/// The chain identity a toolkit operation observes right now, for
/// comparison against recorded state.
pub fn observed_chain(
    config: &crate::system::ToolkitConfig,
    block1_hash: Option<String>,
) -> ChainIdentity {
    ChainIdentity {
        network: config.network_id().to_string(),
        bitcoin_network: config.bitcoin_network_id().to_string(),
        block1_hash,
        labcoat_network_instance_id: config.labcoat_instance_id.clone(),
    }
}

/// Extra deploy-time facts v1 records never carried.
#[derive(Debug, Default)]
pub struct InstanceExtras {
    pub commit_txid: Option<String>,
    pub labcoat_version: Option<String>,
    pub revert_reason: Option<String>,
}

/// Pre-mutation guard for deploy-time dual-write. Fast-path `None` when no
/// v2 state exists for the environment (the feature stays dormant and no
/// lease or directory is created). Otherwise: acquire the lease, load fail
/// closed, and validate the chain identity so `STATE_CHAIN_MISMATCH`
/// aborts BEFORE any broadcast. The held lease is returned so the caller
/// keeps it across the broadcast and the subsequent `record_instance`.
///
/// Recorded-None identity fields are backfilled in memory from the
/// observation; they persist with the next commit.
pub fn deploy_guard(
    root: &Path,
    environment: &str,
    observed: &ChainIdentity,
) -> Result<Option<(StateLease, State)>> {
    if state_backend::load(root, environment)?.is_none() {
        return Ok(None);
    }
    let lease = StateLease::acquire(root, environment)?;
    let Some(mut state) = lease.load()? else {
        return Ok(None);
    };
    validate_chain(&state.chain, observed)?;
    if state.chain.block1_hash.is_none() {
        state.chain.block1_hash = observed.block1_hash.clone();
    }
    if state.chain.labcoat_network_instance_id.is_none() {
        state.chain.labcoat_network_instance_id = observed.labcoat_network_instance_id.clone();
    }
    Ok(Some((lease, state)))
}

/// Append an instance mirroring the v1 record just written, move the
/// resource's active pointer, and commit. History is append-only: the
/// previous instance stays addressable, nothing is erased.
pub fn record_instance(
    lease: &mut StateLease,
    mut state: State,
    contract: &str,
    v1: &lockfile::Deployment,
    origin: InstanceOrigin,
    extras: InstanceExtras,
) -> Result<State> {
    let address = resource_address(contract);
    let resource = state.resources.entry(address).or_insert_with(|| Resource {
        kind: ResourceKind::Contract,
        active_instance: None,
        instances: Vec::new(),
    });
    let instance_id = format!("instance-{}", resource.instances.len() + 1);
    let mut instance = Instance::from_v1(instance_id.clone(), origin, v1);
    instance.commit_txid = extras.commit_txid;
    instance.labcoat_version = extras.labcoat_version;
    instance.revert_reason = extras.revert_reason;
    resource.instances.push(instance);
    resource.active_instance = Some(instance_id);
    let serial = state.serial;
    lease.commit(serial, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> ChainIdentity {
        ChainIdentity {
            network: "labcoat".to_string(),
            bitcoin_network: "regtest".to_string(),
            block1_hash: Some("aa".repeat(32)),
            labcoat_network_instance_id: Some("11111111-1111-4111-8111-111111111111".to_string()),
        }
    }

    fn deployment(chain_id: Option<&str>) -> lockfile::Deployment {
        lockfile::Deployment {
            alkanes_id: "2:1".to_string(),
            wasm_sha256: Some("cc".repeat(32)),
            txid: "00".repeat(32),
            block: None,
            status: "success".to_string(),
            deployed_at: 1_700_000_000_000,
            chain_id: chain_id.map(String::from),
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("labcoat-state-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn lineage_uuids_are_v4_and_unique() {
        let id = new_lineage();
        assert_eq!(id.len(), 36);
        let chars: Vec<char> = id.chars().collect();
        assert_eq!(chars[14], '4');
        assert!(matches!(chars[19], '8' | '9' | 'a' | 'b'));
        assert_ne!(new_lineage(), id);
    }

    #[test]
    fn state_round_trips_with_camel_and_kebab_wire_keys() {
        let mut state = new_state("default", chain(), new_lineage());
        state.serial = 3;
        state.resources.insert(
            resource_address("counter"),
            Resource {
                kind: ResourceKind::Contract,
                active_instance: Some("instance-1".to_string()),
                instances: vec![Instance::from_v1(
                    "instance-1".to_string(),
                    InstanceOrigin::Imported,
                    &deployment(Some("abc")),
                )],
            },
        );
        state.operations.push(Operation {
            operation_id: new_lineage(),
            resource: "contract.counter".to_string(),
            action: OperationAction::Create,
            status: OperationStatus::CommitBroadcast,
            transitions: vec![OperationTransition {
                status: OperationStatus::CommitBroadcast,
                txid: Some("ff".repeat(32)),
                at: 1,
            }],
        });

        let text = to_json_string(&state);
        assert!(text.contains("\"block1Hash\""));
        assert!(text.contains("\"labcoatNetworkInstanceId\""));
        assert!(text.contains("\"instanceId\": \"instance-1\""));
        assert!(text.contains("\"origin\": \"imported\""));
        assert!(text.contains("\"commit-broadcast\""));
        assert!(!text.contains("\"commitTxid\"")); // None fields are omitted

        let parsed = parse(&text, "default").unwrap();
        assert_eq!(parsed.serial, 3);
        assert_eq!(parsed.resources["contract.counter"].instances.len(), 1);
        assert_eq!(parsed.operations.len(), 1);
    }

    #[test]
    fn parse_fails_closed_on_version_corruption_and_wrong_environment() {
        let state = new_state("default", chain(), new_lineage());
        let text = to_json_string(&state);

        assert_eq!(
            parse("{ not json", "default").unwrap_err().code,
            "STATE_INVALID"
        );
        assert_eq!(
            parse(&text.replace("\"version\": 2", "\"version\": 1"), "default")
                .unwrap_err()
                .code,
            "STATE_UNSUPPORTED"
        );
        assert_eq!(
            parse(&text.replace("\"version\": 2", "\"version\": 3"), "default")
                .unwrap_err()
                .code,
            "STATE_UNSUPPORTED"
        );
        // Unknown fields are a schema violation, not ignorable noise.
        let unknown = text.replace("\"serial\": 0", "\"serial\": 0, \"surprise\": true");
        assert_eq!(
            parse(&unknown, "default").unwrap_err().code,
            "STATE_INVALID"
        );
        // A state file moved into another environment's directory.
        assert_eq!(parse(&text, "dev").unwrap_err().code, "STATE_INVALID");
    }

    #[test]
    fn environment_names_are_validated_as_path_components() {
        assert!(validate_environment_name("default").is_ok());
        assert!(validate_environment_name("dev_2-a").is_ok());
        assert!(validate_environment_name("").is_err());
        assert!(validate_environment_name("../escape").is_err());
        assert!(validate_environment_name("a/b").is_err());
        assert!(validate_environment_name(".").is_err());
    }

    #[test]
    fn chain_validation_rejects_resets_and_unverifiable_identity() {
        let recorded = chain();
        assert!(validate_chain(&recorded, &recorded).is_ok());

        let mut other_network = recorded.clone();
        other_network.network = "signet".to_string();
        other_network.bitcoin_network = "signet".to_string();
        assert_eq!(
            validate_chain(&recorded, &other_network).unwrap_err().code,
            "STATE_CHAIN_MISMATCH"
        );

        let mut reset_instance = recorded.clone();
        reset_instance.labcoat_network_instance_id = Some("other".to_string());
        assert_eq!(
            validate_chain(&recorded, &reset_instance).unwrap_err().code,
            "STATE_CHAIN_MISMATCH"
        );

        let mut reset_chain = recorded.clone();
        reset_chain.block1_hash = Some("bb".repeat(32));
        assert_eq!(
            validate_chain(&recorded, &reset_chain).unwrap_err().code,
            "STATE_CHAIN_MISMATCH"
        );

        // Recorded identity that can no longer be observed fails closed.
        let mut unobservable = recorded.clone();
        unobservable.block1_hash = None;
        unobservable.labcoat_network_instance_id = None;
        assert_eq!(
            validate_chain(&recorded, &unobservable).unwrap_err().code,
            "STATE_CHAIN_MISMATCH"
        );

        // Recorded-None fields are backfillable, not mismatches.
        let mut sparse = recorded.clone();
        sparse.block1_hash = None;
        sparse.labcoat_network_instance_id = None;
        assert!(validate_chain(&sparse, &recorded).is_ok());
    }

    #[test]
    fn v1_records_round_trip_through_instances_verbatim() {
        // Full record, revert status.
        let mut full = deployment(Some("abc"));
        full.status = "revert".to_string();
        let instance = Instance::from_v1("instance-1".into(), InstanceOrigin::Imported, &full);
        assert_eq!(instance.to_v1(), full);
        assert!(instance.commit_txid.is_none());
        assert!(instance.labcoat_version.is_none());

        // Sparse pre-chain-id record with the "adopted" sentinel.
        let sparse = lockfile::Deployment {
            alkanes_id: "4:99".to_string(),
            wasm_sha256: None,
            txid: "adopted".to_string(),
            block: None,
            status: "unknown".to_string(),
            deployed_at: 0,
            chain_id: None,
        };
        let instance = Instance::from_v1("instance-1".into(), InstanceOrigin::Adopted, &sparse);
        assert_eq!(instance.to_v1(), sparse);
    }

    #[test]
    fn migration_imports_only_the_selected_network() {
        let mut lock = lockfile::Lockfile::default();
        lock.version = 1;
        let mut labcoat = BTreeMap::new();
        labcoat.insert("counter".to_string(), deployment(Some("abc")));
        let mut reverted = deployment(Some("abc"));
        reverted.status = "revert".to_string();
        labcoat.insert("token".to_string(), reverted);
        lock.networks.insert("labcoat".to_string(), labcoat);
        let mut signet = BTreeMap::new();
        signet.insert("counter".to_string(), deployment(None));
        lock.networks.insert("signet".to_string(), signet);

        let state = migrate_v1(&MigrationInputs {
            lockfile: &lock,
            network: "labcoat",
            environment: "default",
            chain: chain(),
            lineage: new_lineage(),
        });
        assert_eq!(state.serial, 0);
        assert_eq!(state.resources.len(), 2);
        let token = &state.resources["contract.token"];
        assert_eq!(token.active_instance.as_deref(), Some("instance-1"));
        assert_eq!(token.instances[0].origin, InstanceOrigin::Imported);
        assert_eq!(token.instances[0].status, "revert");
        assert!(state.operations.is_empty());
    }

    #[test]
    fn migrate_backs_up_round_trips_and_refuses_to_run_twice() {
        let root = temp_root("migrate");
        let mut lock = lockfile::Lockfile::default();
        lock.version = 1;
        let mut labcoat = BTreeMap::new();
        labcoat.insert("counter".to_string(), deployment(Some("abc")));
        lock.networks.insert("labcoat".to_string(), labcoat);
        let mut signet = BTreeMap::new();
        signet.insert("counter".to_string(), deployment(None));
        lock.networks.insert("signet".to_string(), signet);
        lockfile::save(&root, &lock).unwrap();
        let original = std::fs::read(root.join(lockfile::LOCKFILE)).unwrap();

        let outcome = migrate(
            &root,
            "default",
            "labcoat",
            chain(),
            new_lineage(),
            1_755_000_000_000,
        )
        .unwrap();
        assert_eq!(outcome.resources, 1);
        assert_eq!(outcome.instances, 1);
        assert_eq!(outcome.serial, 1);
        assert!(outcome.lockfile_regenerated);

        // Backup holds the pre-migration bytes; the regenerated lockfile is
        // byte-identical, including the untouched signet subtree.
        let backup = outcome.backup.unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), original);
        assert_eq!(
            std::fs::read(root.join(lockfile::LOCKFILE)).unwrap(),
            original
        );

        let err = migrate(
            &root,
            "default",
            "labcoat",
            chain(),
            new_lineage(),
            1_755_000_000_001,
        )
        .unwrap_err();
        assert_eq!(err.code, "STATE_CONFLICT");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn migrate_without_a_lockfile_initializes_empty_state() {
        let root = temp_root("migrate-empty");
        let outcome = migrate(&root, "default", "labcoat", chain(), new_lineage(), 0).unwrap();
        assert_eq!(outcome.resources, 0);
        assert!(outcome.backup.is_none());
        assert!(!outcome.lockfile_regenerated);
        assert!(!root.join(lockfile::LOCKFILE).exists());
        assert!(state_backend::load(&root, "default").unwrap().is_some());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn deploy_guard_is_dormant_without_state_and_rejects_resets() {
        let root = temp_root("guard");
        // Dormant: no state, no directories created, no lease taken.
        assert!(deploy_guard(&root, "default", &chain()).unwrap().is_none());
        assert!(!root.join(".labcoat").exists());

        migrate(&root, "default", "labcoat", chain(), new_lineage(), 0).unwrap();

        let (lease, state) = deploy_guard(&root, "default", &chain()).unwrap().unwrap();
        assert_eq!(state.serial, 1);
        drop(lease);

        // A reset chain is rejected before any mutation, and the failed
        // guard releases the lease.
        let mut reset = chain();
        reset.labcoat_network_instance_id = Some("regenerated".to_string());
        assert_eq!(
            deploy_guard(&root, "default", &reset).unwrap_err().code,
            "STATE_CHAIN_MISMATCH"
        );
        assert!(deploy_guard(&root, "default", &chain()).unwrap().is_some());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn record_instance_appends_history_and_moves_the_active_pointer() {
        let root = temp_root("record");
        let mut lock = lockfile::Lockfile::default();
        lock.version = 1;
        let mut labcoat = BTreeMap::new();
        labcoat.insert("counter".to_string(), deployment(Some("abc")));
        lock.networks.insert("labcoat".to_string(), labcoat);
        lockfile::save(&root, &lock).unwrap();
        migrate(&root, "default", "labcoat", chain(), new_lineage(), 0).unwrap();

        let (mut lease, state) = deploy_guard(&root, "default", &chain()).unwrap().unwrap();
        let mut redeploy = deployment(Some("abc"));
        redeploy.alkanes_id = "2:9".to_string();
        let state = record_instance(
            &mut lease,
            state,
            "counter",
            &redeploy,
            InstanceOrigin::Deployed,
            InstanceExtras {
                commit_txid: Some("dd".repeat(32)),
                labcoat_version: Some("0.2.0".to_string()),
                revert_reason: None,
            },
        )
        .unwrap();
        drop(lease);

        assert_eq!(state.serial, 2);
        let resource = &state.resources["contract.counter"];
        assert_eq!(resource.instances.len(), 2);
        assert_eq!(resource.active_instance.as_deref(), Some("instance-2"));
        assert_eq!(resource.instances[0].alkanes_id, "2:1"); // history preserved
        assert_eq!(resource.instances[1].origin, InstanceOrigin::Deployed);
        assert_eq!(
            resource.instances[1].commit_txid.as_deref(),
            Some("dd".repeat(32)).as_deref()
        );

        let reloaded = state_backend::load(&root, "default").unwrap().unwrap();
        assert_eq!(reloaded.serial, 2);
        std::fs::remove_dir_all(root).ok();
    }
}
