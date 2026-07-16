use crate::contract::{CmdResult, EnvelopeError};
use std::path::{Path, PathBuf};

pub fn run(package: Option<&str>) -> CmdResult {
    let project_root = std::env::current_dir().map_err(|e| EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: e.to_string(),
        hint: "run the command from a Labcoat project directory",
    })?;
    let workspace = labcoat_core::workspace::discover(&project_root).map_err(core_error)?;
    if package.is_some() {
        labcoat_core::workspace::select(&workspace, package).map_err(core_error)?;
    }
    let artifact_dir = workspace.root.join(".labcoat/test-artifacts");
    std::fs::create_dir_all(&artifact_dir).map_err(io_error)?;
    let artifacts = labcoat_core::compile::compile_packages(
        &workspace,
        &workspace.contracts,
        &artifact_dir,
        "wasm32-wasip1",
    )
    .map_err(core_error)?;

    let mut command = std::process::Command::new("cargo");
    if let Some(path) = local_labcoat_test_path() {
        let escaped = path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        command
            .arg("--config")
            .arg(format!("patch.crates-io.labcoat-test.path=\"{escaped}\""));
    }
    command.arg("test");
    if let Some(package) = package {
        let target = labcoat_core::workspace::host_test_for_package(&workspace, package)
            .ok_or_else(|| EnvelopeError {
                code: "CONFIG_INVALID",
                message: format!("no host integration test found at tests/{package}.rs"),
                hint: "create tests/<package>.rs for the selected contract",
            })?;
        command.args(["--test", &target.name]);
    } else {
        command.arg("--tests");
    }
    let output = command
        .current_dir(&workspace.root)
        .env("LABCOAT_TEST_ARTIFACT_DIR", &artifact_dir)
        .output()
        .map_err(|e| EnvelopeError {
            code: "TOOLKIT_ERROR",
            message: format!("failed to run cargo test: {}", e),
            hint: "install Cargo and run `labcoat doctor`",
        })?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(EnvelopeError {
            code: "TEST_FAILED",
            message: format!("{stdout}{stderr}"),
            hint: "fix the failing Rust tests under tests/ and re-run `labcoat test`",
        });
    }

    Ok(serde_json::json!({
        "contracts": artifacts,
        "artifactDir": artifact_dir,
        "passed": true,
        "output": String::from_utf8_lossy(&output.stdout),
    }))
}

fn core_error(error: labcoat_core::LabcoatError) -> EnvelopeError {
    EnvelopeError {
        code: error.code,
        message: error.message,
        hint: error.hint,
    }
}

/// Resolve the unpublished test harness while developing Labcoat from source.
///
/// Release builds normally return `None` because their build checkout no longer
/// exists, so generated projects resolve the version pinned in Cargo.toml from
/// crates.io. `LABCOAT_TEST_CRATE_PATH` remains available for CI and packagers.
fn local_labcoat_test_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("LABCOAT_TEST_CRATE_PATH") {
        return Some(PathBuf::from(path));
    }

    sibling_test_crate(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn sibling_test_crate(cli_manifest_dir: &Path) -> Option<PathBuf> {
    let candidate = cli_manifest_dir.parent()?.join("labcoat-test");
    candidate.join("Cargo.toml").is_file().then_some(candidate)
}

fn io_error(error: std::io::Error) -> EnvelopeError {
    EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: error.to_string(),
        hint: "check project permissions and available disk space",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_sibling_test_crate_for_source_builds() {
        let root = std::env::temp_dir().join(format!(
            "labcoat-test-crate-discovery-{}",
            std::process::id()
        ));
        let cli = root.join("labcoat-cli");
        let harness = root.join("labcoat-test");
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&cli).unwrap();
        std::fs::create_dir_all(&harness).unwrap();

        assert_eq!(sibling_test_crate(&cli), None);
        std::fs::write(
            harness.join("Cargo.toml"),
            "[package]\nname='labcoat-test'\n",
        )
        .unwrap();
        assert_eq!(sibling_test_crate(&cli), Some(harness));

        std::fs::remove_dir_all(root).ok();
    }
}
