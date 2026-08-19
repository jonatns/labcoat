//! Persistent Labcoat Network chain-instance identity.
//!
//! A UUID stored inside the qubitcoin data directory, so `labcoat reset`
//! (which removes that directory, see `ProcessManager::reset_data`)
//! regenerates it for free. Consumers record the UUID alongside chain
//! state and compare it later to detect that a reset happened even when
//! block hashes are unavailable.

use crate::config::get_qubitcoin_dir;
use std::path::{Path, PathBuf};

/// Location of the instance-id file: one line of plain text (the UUID).
pub fn instance_id_path() -> PathBuf {
    get_qubitcoin_dir().join("labcoat-instance-id")
}

/// Read-or-create the chain-instance UUID.
pub fn instance_id() -> Result<String, String> {
    instance_id_at(&instance_id_path())
}

/// Read the chain-instance UUID without creating one.
pub fn instance_id_if_exists() -> Option<String> {
    read_at(&instance_id_path())
}

fn read_at(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let id = text.trim();
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

fn instance_id_at(path: &Path) -> Result<String, String> {
    if let Some(id) = read_at(path) {
        return Ok(id);
    }
    let id = uuid_v4();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create instance-id directory: {e}"))?;
    }
    // Same tmp+rename discipline as the faucet state beside it, so a crash
    // mid-write never leaves a truncated id.
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, format!("{id}\n"))
        .map_err(|e| format!("Failed to write instance id: {e}"))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("Failed to persist instance id: {e}"))?;
    Ok(id)
}

/// A random RFC 4122 version-4 UUID, without pulling in the `uuid` crate.
fn uuid_v4() -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("labcoat-instance-id-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn uuid_v4_has_version_and_variant_bits() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        let chars: Vec<char> = id.chars().collect();
        assert_eq!(chars[8], '-');
        assert_eq!(chars[13], '-');
        assert_eq!(chars[18], '-');
        assert_eq!(chars[23], '-');
        assert_eq!(chars[14], '4');
        assert!(matches!(chars[19], '8' | '9' | 'a' | 'b'));
        assert_ne!(uuid_v4(), id);
    }

    #[test]
    fn instance_id_is_created_once_and_regenerates_after_reset() {
        let root = temp_root("lifecycle");
        let path = root.join("labcoat-instance-id");

        let first = instance_id_at(&path).unwrap();
        assert_eq!(instance_id_at(&path).unwrap(), first);

        // A reset deletes the data directory; a new id must appear.
        std::fs::remove_file(&path).unwrap();
        let second = instance_id_at(&path).unwrap();
        assert_ne!(second, first);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn read_at_never_creates_and_ignores_empty_files() {
        let root = temp_root("readonly");
        let path = root.join("labcoat-instance-id");
        assert_eq!(read_at(&path), None);
        assert!(!path.exists());

        std::fs::write(&path, "\n").unwrap();
        assert_eq!(read_at(&path), None);

        std::fs::write(&path, "abc-123\n").unwrap();
        assert_eq!(read_at(&path).as_deref(), Some("abc-123"));
        std::fs::remove_dir_all(root).ok();
    }
}
