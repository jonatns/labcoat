use crate::contract::{CmdResult, Ctx, EnvelopeError};
use std::path::{Path, PathBuf};

/// `labcoat test --e2e [filter]` — reset the Labcoat Network (opt-out with
/// `--no-reset`), apply the `alkanes.hcl` manifest when present, then run
/// the project's ignored `tests/e2e.rs` tests with the context env set.
pub async fn run_e2e(ctx: &Ctx, filter: Option<&str>, no_reset: bool) -> CmdResult {
    if ctx.config.network != labcoat_core::NetworkTarget::Labcoat {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: format!(
                "e2e tests only run against Labcoat Network, not `{}`",
                ctx.config.network_id()
            ),
            hint: "drop the --network override; e2e runs reset the chain and must never touch a shared network",
        });
    }
    let project_root = std::env::current_dir().map_err(|e| EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: e.to_string(),
        hint: "run the command from a Labcoat project directory",
    })?;
    let e2e_target = project_root.join("tests/e2e.rs");
    if !e2e_target.is_file() {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: "no e2e test target at tests/e2e.rs".into(),
            hint: "create tests/e2e.rs with #[ignore] tests using labcoat_test::e2e::E2e",
        });
    }

    let mut reset = false;
    {
        let mut network = isomer_core::LabcoatNetwork::new();
        if !no_reset {
            network
                .ensure_binaries(|_, _| {})
                .await
                .map_err(|message| EnvelopeError {
                    code: "TOOLKIT_ERROR",
                    message,
                    hint: "check network access to the binary hosts",
                })?;
            network.reset().map_err(|message| EnvelopeError {
                code: "TOOLKIT_ERROR",
                message,
                hint: "run `labcoat doctor`",
            })?;
            network.start().map_err(|message| EnvelopeError {
                code: "TOOLKIT_ERROR",
                message,
                hint: "run `labcoat doctor` and `labcoat logs`",
            })?;
            reset = true;
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let status = network.status().await;
            if status.is_ready {
                break;
            }
            if std::time::Instant::now() > deadline {
                std::mem::forget(network);
                return Err(EnvelopeError {
                    code: "RPC_UNREACHABLE",
                    message: "Labcoat Network not ready after 120s".into(),
                    hint: "inspect `labcoat logs` and `labcoat status`",
                });
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        // The network must outlive this run; dropping the handle would stop
        // the children a reset spawned.
        std::mem::forget(network);
    }

    // A reset leaves the project wallet empty; make sure it exists and
    // holds mature regtest BTC before the manifest deploys anything.
    {
        let passphrase = ctx.passphrase();
        let address = async {
            ctx.config.require_passphrase_policy(&passphrase)?;
            let mut provider =
                labcoat_core::system::connect(&ctx.config, passphrase.clone(), false).await?;
            if !ctx.config.wallet_file.exists() {
                labcoat_core::wallet::init(&mut provider, &ctx.config, None, passphrase.clone())
                    .await?;
            }
            let provider =
                labcoat_core::system::connect(&ctx.config, passphrase.clone(), true).await?;
            labcoat_core::wallet::primary_address(&provider).await
        }
        .await
        .map_err(core_error)?;
        let network = isomer_core::LabcoatNetwork::new();
        network
            .fund(&address, 2.0)
            .await
            .map_err(|message| EnvelopeError {
                code: "TOOLKIT_ERROR",
                message,
                hint: "check `labcoat status` and the faucet logs",
            })?;
        network
            .mine(1, None)
            .await
            .map_err(|message| EnvelopeError {
                code: "TOOLKIT_ERROR",
                message,
                hint: "check `labcoat status`",
            })?;
    }

    let manifest_path = project_root.join(labcoat_core::manifest::MANIFEST);
    let applied = if manifest_path.is_file() {
        let report = labcoat_core::apply::apply(
            &ctx.config,
            &ctx.signer_spec().map_err(core_error)?,
            &project_root,
            &manifest_path,
        )
        .await
        .map_err(core_error)?;
        Some(serde_json::to_value(report).expect("serializable apply report"))
    } else {
        None
    };

    let labcoat_bin = std::env::current_exe().map_err(io_error)?;
    let mut command = std::process::Command::new("cargo");
    if let Some(path) = local_labcoat_test_path() {
        let escaped = path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        command.arg("--config").arg(format!(
            "patch.'https://github.com/jonatns/labcoat'.labcoat-test.path=\"{escaped}\""
        ));
    }
    command.args(["test", "--test", "e2e"]);
    command.args(["--", "--ignored"]);
    if let Some(filter) = filter {
        command.arg(filter);
    }
    let output = command
        .current_dir(&project_root)
        .env("LABCOAT_E2E_BIN", &labcoat_bin)
        .env("LABCOAT_E2E_ROOT", &project_root)
        .output()
        .map_err(|e| EnvelopeError {
            code: "TOOLKIT_ERROR",
            message: format!("failed to run cargo test: {e}"),
            hint: "install Cargo and run `labcoat doctor`",
        })?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(EnvelopeError {
            code: "TEST_FAILED",
            message: format!("{stdout}{stderr}"),
            hint: "fix the failing e2e tests under tests/e2e.rs and re-run `labcoat test --e2e`",
        });
    }

    Ok(serde_json::json!({
        "e2e": true,
        "reset": reset,
        "applied": applied,
        "passed": true,
        "output": String::from_utf8_lossy(&output.stdout),
    }))
}

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
    let _local_harness =
        labcoat_core::compile::explicit_local_labcoat_test_override(&workspace.root)
            .map_err(core_error)?;
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
        command.arg("--config").arg(format!(
            "patch.'https://github.com/jonatns/labcoat'.labcoat-test.path=\"{escaped}\""
        ));
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
            message: format!("failed to run cargo test: {e}"),
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
/// exists, so generated projects resolve the harness from their matching
/// `cli-vX.Y.Z` Git tag. `LABCOAT_TEST_CRATE_PATH` remains available for CI and
/// packagers.
fn local_labcoat_test_path() -> Option<PathBuf> {
    if std::env::var_os("LABCOAT_TEST_CRATE_PATH").is_some() {
        return None;
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
