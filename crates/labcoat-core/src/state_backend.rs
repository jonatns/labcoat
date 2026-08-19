//! Locked, atomic, fail-closed persistence for version-2 durable state.
//!
//! Layout per environment, under the project root:
//!
//! ```text
//! .labcoat/state/<environment>/state.json   the state (state.rs schema)
//! .labcoat/state/<environment>/state.lock   the environment lease
//! .labcoat/state/<environment>/backups/     state.json.prev + migrate backups
//! ```
//!
//! The flat `.labcoat/state/<network>.json` files beside these directories
//! are the apply call journal (`apply.rs`) and are not touched here.
//!
//! The remote-backend trait from the durable-state design is deferred to
//! the first remote backend: with one implementation it is ceremony, and
//! local compare-and-swap is only meaningful on a held lease anyway. The
//! lease + serial-CAS semantics a remote backend needs are what
//! [`StateLease::commit`] implements.

use crate::error::{LabcoatError, Result};
use crate::state::{self, State, BACKUPS_DIR, LOCK_FILE, STATE_FILE};
use std::path::{Path, PathBuf};

pub fn state_dir(root: &Path, environment: &str) -> Result<PathBuf> {
    state::validate_environment_name(environment)?;
    Ok(root.join(".labcoat").join("state").join(environment))
}

pub fn state_path(root: &Path, environment: &str) -> Result<PathBuf> {
    Ok(state_dir(root, environment)?.join(STATE_FILE))
}

/// Fail-closed read without the lease, for read-only commands. Safe
/// because writers replace the file atomically, so a reader sees either
/// the old or the new state — never a partial write.
pub fn load(root: &Path, environment: &str) -> Result<Option<State>> {
    load_at(&state_path(root, environment)?, environment)
}

fn load_at(path: &Path, environment: &str) -> Result<Option<State>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(LabcoatError::new(
                "STATE_INVALID",
                format!("cannot read {}: {}", path.display(), e),
                "check permissions on .labcoat/state",
            ))
        }
    };
    state::parse(&text, environment).map(Some)
}

/// An exclusive advisory OS lock on the environment's `state.lock`. The
/// kernel releases it when the file handle closes — on drop or on crash —
/// so a dead process can never wedge the environment.
#[derive(Debug)]
pub struct StateLease {
    // Held for its open file description; the forgotten write guard in
    // `acquire` keeps the OS lock alive until this closes.
    _lock: fd_lock::RwLock<std::fs::File>,
    dir: PathBuf,
    environment: String,
}

impl StateLease {
    pub fn acquire(root: &Path, environment: &str) -> Result<StateLease> {
        let dir = state_dir(root, environment)?;
        let io_err = |e: std::io::Error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot prepare {}: {}", dir.display(), e),
                "check permissions on .labcoat/state",
            )
        };
        std::fs::create_dir_all(&dir).map_err(io_err)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join(LOCK_FILE))
            .map_err(io_err)?;
        let mut lock = fd_lock::RwLock::new(file);
        match lock.try_write() {
            Ok(guard) => {
                // The guard only borrows the RwLock; forgetting it keeps
                // the OS lock held by the open file description until this
                // lease drops. Never unlocked early, never self-referential.
                std::mem::forget(guard);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(LabcoatError::new(
                    "STATE_LOCKED",
                    format!(
                        "another labcoat process holds the lease for environment '{environment}'"
                    ),
                    "wait for the other process; a crashed holder releases the lease automatically",
                ))
            }
            Err(e) => return Err(io_err(e)),
        }
        Ok(StateLease {
            _lock: lock,
            dir,
            environment: environment.to_string(),
        })
    }

    pub fn environment(&self) -> &str {
        &self.environment
    }

    pub fn load(&self) -> Result<Option<State>> {
        load_at(&self.dir.join(STATE_FILE), &self.environment)
    }

    /// Compare-and-swap commit. In order: re-load the current file (a
    /// corrupt current state fails the commit — corruption is never
    /// overwritten), compare its serial against `expected_serial`, bump the
    /// serial exactly once, write a temp file, fsync it, keep the previous
    /// state as `backups/state.json.prev`, rename into place, and fsync
    /// the directory. A failure at any step removes the temp file and
    /// leaves the previous state intact.
    pub fn commit(&mut self, expected_serial: u64, mut state: State) -> Result<State> {
        let path = self.dir.join(STATE_FILE);
        let current_serial = self.load()?.map(|s| s.serial).unwrap_or(0);
        if current_serial != expected_serial {
            return Err(LabcoatError::new(
                "STATE_CONFLICT",
                format!("durable state serial is {current_serial}, expected {expected_serial}"),
                "durable state changed underneath this command; re-run it against current state",
            ));
        }
        state.serial = expected_serial + 1;
        let text = state::to_json_string(&state);

        let tmp = self
            .dir
            .join(format!(".{}.tmp-{}", STATE_FILE, std::process::id()));
        let io_err = |e: std::io::Error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot write durable state: {e}"),
                "check permissions on .labcoat/state",
            )
        };
        let write = (|| {
            let mut file = std::fs::File::create(&tmp)?;
            use std::io::Write;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            if path.exists() {
                let backups = self.dir.join(BACKUPS_DIR);
                std::fs::create_dir_all(&backups)?;
                std::fs::copy(&path, backups.join(format!("{STATE_FILE}.prev")))?;
            }
            std::fs::rename(&tmp, &path)
        })();
        if write.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        write.map_err(io_err)?;
        // Directory fsync makes the rename itself durable. Best-effort:
        // not every filesystem supports fsync on a directory handle, and
        // the data is already safe in either the old or the new file.
        #[cfg(unix)]
        if let Ok(dir) = std::fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(state)
    }

    /// Timestamped copy of an arbitrary file into this environment's
    /// backups directory (used by `state migrate` for labcoat.lock).
    pub fn backup_file(&self, source: &Path, label: &str, now_millis: u64) -> Result<PathBuf> {
        let backups = self.dir.join(BACKUPS_DIR);
        let io_err = |e: std::io::Error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot back up {}: {}", source.display(), e),
                "check permissions on .labcoat/state",
            )
        };
        std::fs::create_dir_all(&backups).map_err(io_err)?;
        let dest = backups.join(format!("{label}.{now_millis}.bak"));
        std::fs::copy(source, &dest).map_err(io_err)?;
        let file = std::fs::File::open(&dest).map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ChainIdentity;

    fn chain() -> ChainIdentity {
        ChainIdentity {
            network: "labcoat".to_string(),
            bitcoin_network: "regtest".to_string(),
            block1_hash: Some("aa".repeat(32)),
            labcoat_network_instance_id: None,
        }
    }

    fn fresh_state() -> State {
        state::new_state("default", chain(), state::new_lineage())
    }

    fn temp_root(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("labcoat-statebackend-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn missing_state_is_none_but_corruption_is_an_error() {
        let root = temp_root("failclosed");
        assert!(load(&root, "default").unwrap().is_none());

        let dir = state_dir(&root, "default").unwrap();
        std::fs::create_dir_all(&dir).unwrap();

        // Truncated/corrupt state must never read as empty.
        std::fs::write(dir.join(STATE_FILE), "{\"version\": 2, \"lineage").unwrap();
        assert_eq!(load(&root, "default").unwrap_err().code, "STATE_INVALID");

        std::fs::write(dir.join(STATE_FILE), "{ not json").unwrap();
        assert_eq!(load(&root, "default").unwrap_err().code, "STATE_INVALID");

        std::fs::write(dir.join(STATE_FILE), "{\"version\": 9, \"unknown\": true}").unwrap();
        assert_eq!(
            load(&root, "default").unwrap_err().code,
            "STATE_UNSUPPORTED"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn commit_is_atomic_increments_serial_once_and_keeps_a_backup() {
        let root = temp_root("commit");
        let mut lease = StateLease::acquire(&root, "default").unwrap();

        let first = lease.commit(0, fresh_state()).unwrap();
        assert_eq!(first.serial, 1);
        let first_bytes = std::fs::read(state_path(&root, "default").unwrap()).unwrap();

        // Wrong expected serial is a conflict, and the file is untouched.
        assert_eq!(
            lease.commit(0, fresh_state()).unwrap_err().code,
            "STATE_CONFLICT"
        );
        assert_eq!(
            std::fs::read(state_path(&root, "default").unwrap()).unwrap(),
            first_bytes
        );

        let second = lease.commit(1, first.clone()).unwrap();
        assert_eq!(second.serial, 2);

        // The previous state survives as the last-known-good backup, and no
        // temp files are left behind.
        let backup = state_dir(&root, "default")
            .unwrap()
            .join(BACKUPS_DIR)
            .join(format!("{STATE_FILE}.prev"));
        assert_eq!(std::fs::read(backup).unwrap(), first_bytes);
        let stray: Vec<_> = std::fs::read_dir(state_dir(&root, "default").unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(stray.is_empty(), "leftover temp files: {stray:?}");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn commit_never_overwrites_a_corrupt_current_state() {
        let root = temp_root("no-clobber");
        let mut lease = StateLease::acquire(&root, "default").unwrap();
        lease.commit(0, fresh_state()).unwrap();

        let path = state_path(&root, "default").unwrap();
        std::fs::write(&path, "{ corrupted").unwrap();
        assert_eq!(
            lease.commit(1, fresh_state()).unwrap_err().code,
            "STATE_INVALID"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ corrupted");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn hand_constructed_crash_points_leave_old_or_new_state() {
        // A crash between temp-write and rename: stale temp beside a valid
        // state. The old state wins and the next commit still succeeds.
        let root = temp_root("crash-tmp");
        let mut lease = StateLease::acquire(&root, "default").unwrap();
        let committed = lease.commit(0, fresh_state()).unwrap();
        let dir = state_dir(&root, "default").unwrap();
        std::fs::write(
            dir.join(format!(".{STATE_FILE}.tmp-99999")),
            "{ half a write",
        )
        .unwrap();

        let loaded = lease.load().unwrap().unwrap();
        assert_eq!(loaded.serial, 1);
        assert_eq!(lease.commit(1, committed).unwrap().serial, 2);

        // A crash before the first rename: temp file only. "Old" state was
        // none, so load reports none rather than reading the partial file.
        let root2 = temp_root("crash-first");
        let dir2 = state_dir(&root2, "default").unwrap();
        std::fs::create_dir_all(&dir2).unwrap();
        std::fs::write(
            dir2.join(format!(".{STATE_FILE}.tmp-99999")),
            "{ half a write",
        )
        .unwrap();
        assert!(load(&root2, "default").unwrap().is_none());

        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(root2).ok();
    }

    #[test]
    fn lease_conflicts_across_handles_and_releases_on_drop() {
        let root = temp_root("lease");
        let first = StateLease::acquire(&root, "default").unwrap();
        // A second acquire opens its own file description, so the OS lock
        // conflicts even within one process.
        assert_eq!(
            StateLease::acquire(&root, "default").unwrap_err().code,
            "STATE_LOCKED"
        );
        // Environments are independent leases.
        assert!(StateLease::acquire(&root, "dev").is_ok());

        drop(first);
        assert!(StateLease::acquire(&root, "default").is_ok());
        std::fs::remove_dir_all(root).ok();
    }

    /// Helper for the cross-process test below: inert unless the driver
    /// re-executes this test binary with LABCOAT_LEASE_DIR set.
    #[test]
    fn lease_contention_helper() {
        let Some(dir) = std::env::var_os("LABCOAT_LEASE_DIR") else {
            return;
        };
        let root = PathBuf::from(dir);
        let outcome = match StateLease::acquire(&root, "default") {
            Ok(_) => "acquired".to_string(),
            Err(e) => e.code.to_string(),
        };
        std::fs::write(root.join("lease-result"), outcome).unwrap();
    }

    #[test]
    fn lease_blocks_a_second_process() {
        let root = temp_root("lease-xproc");
        let _held = StateLease::acquire(&root, "default").unwrap();

        let exe = std::env::current_exe().unwrap();
        let output = std::process::Command::new(exe)
            .args([
                "state_backend::tests::lease_contention_helper",
                "--exact",
                "--test-threads=1",
            ])
            .env("LABCOAT_LEASE_DIR", &root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "helper run failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(root.join("lease-result")).unwrap(),
            "STATE_LOCKED"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
