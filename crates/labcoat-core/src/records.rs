//! Where a project keeps its deployment records: `labcoat.lock`, the run
//! files under `deployments/`, durable state under `.labcoat/state/`, and
//! the lock that serializes writes to them.
//!
//! They live in the project root. `LABCOAT_RECORDS_DIR` moves all of them
//! together, which is how `labcoat test --e2e` records what it deploys on
//! its own chain without replacing the records of the chain the developer
//! works on. The variable reaches every `labcoat` command the tests run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RECORDS_DIR_ENV: &str = "LABCOAT_RECORDS_DIR";

/// The directory holding the records of the project at `root`.
pub fn dir(root: &Path) -> PathBuf {
    resolve(root, std::env::var_os(RECORDS_DIR_ENV))
}

fn resolve(root: &Path, redirected: Option<OsString>) -> PathBuf {
    match redirected.filter(|dir| !dir.is_empty()) {
        // A relative directory is the project's, wherever the command runs.
        Some(dir) => root.join(dir),
        None => root.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_stay_in_the_project_unless_redirected() {
        let root = Path::new("/project");
        assert_eq!(resolve(root, None), root);
        assert_eq!(resolve(root, Some(OsString::new())), root);
        assert_eq!(
            resolve(root, Some("/elsewhere/records".into())),
            Path::new("/elsewhere/records")
        );
        assert_eq!(
            resolve(root, Some(".labcoat/e2e/records".into())),
            Path::new("/project/.labcoat/e2e/records")
        );
    }
}
