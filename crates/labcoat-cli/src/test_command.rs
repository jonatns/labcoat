use crate::contract::{CmdResult, EnvelopeError};
use std::path::PathBuf;

pub fn run(path: Option<&str>) -> CmdResult {
    let sources = contract_sources(path)?;
    let project_root = std::env::current_dir().map_err(|e| EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: e.to_string(),
        hint: "run the command from a Labcoat project directory",
    })?;
    let artifact_dir = project_root.join(".labcoat/test-artifacts");
    std::fs::create_dir_all(&artifact_dir).map_err(io_error)?;

    let mut artifacts = Vec::new();
    for source in &sources {
        let outcome =
            labcoat_core::compile::compile_for_target(source, None, &artifact_dir, "wasm32-wasip1")
                .map_err(|e| EnvelopeError {
                    code: e.code,
                    message: e.message,
                    hint: e.hint,
                })?;
        artifacts.push(outcome);
    }

    let mut command = std::process::Command::new("cargo");
    if let Ok(path) = std::env::var("LABCOAT_TEST_CRATE_PATH") {
        let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
        command.args([
            "--config",
            &format!("patch.crates-io.labcoat-test.path=\"{}\"", escaped),
        ]);
    }
    command.args(["test", "--tests"]);
    let output = command
        .current_dir(&project_root)
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

fn contract_sources(path: Option<&str>) -> Result<Vec<PathBuf>, EnvelopeError> {
    let path = PathBuf::from(path.unwrap_or("contracts"));
    if path.is_file() {
        return Ok(vec![path]);
    }
    if !path.is_dir() {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: format!("contract path {} does not exist", path.display()),
            hint: "pass a .rs contract or create a contracts/ directory",
        });
    }
    let mut sources: Vec<PathBuf> = std::fs::read_dir(&path)
        .map_err(io_error)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .collect();
    sources.sort();
    if sources.is_empty() {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: format!("no .rs contracts found in {}", path.display()),
            hint: "add a contract source or pass its path explicitly",
        });
    }
    Ok(sources)
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
    fn discovers_sorted_contract_sources() {
        let root = std::env::temp_dir().join(format!("labcoat-sources-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("B.rs"), "").unwrap();
        std::fs::write(root.join("A.rs"), "").unwrap();
        std::fs::write(root.join("ignore.txt"), "").unwrap();
        let sources = contract_sources(root.to_str()).unwrap();
        assert_eq!(sources[0].file_name().unwrap(), "A.rs");
        assert_eq!(sources[1].file_name().unwrap(), "B.rs");
        std::fs::remove_dir_all(root).ok();
    }
}
