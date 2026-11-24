use crate::contract::{CmdResult, Ctx, EnvelopeError};

/// `labcoat test --e2e [filter]` — reset the Labcoat Network (opt-out with
/// `--no-reset`), apply the `deploy.rs` deployment script when present,
/// then run the project's ignored `tests/e2e.rs` tests with the context
/// env set.
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
    // holds mature regtest BTC before the script deploys anything.
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

    let applied = if project_root
        .join(labcoat_core::deployment::SCRIPT)
        .is_file()
    {
        let graph = crate::deploy_script::load_graph(&project_root, ctx.config.network_id())
            .map_err(core_error)?;
        let report = labcoat_core::apply::apply(
            &ctx.config,
            &ctx.signer_spec().map_err(core_error)?,
            &project_root,
            &graph,
            &labcoat_core::apply::Options::default(),
        )
        .await
        .map_err(core_error)?;
        Some(serde_json::to_value(report).expect("serializable apply report"))
    } else {
        None
    };

    // Honor LABCOAT_TEST_CRATE_PATH like `labcoat test` does, so CI and
    // packagers can run e2e tests before the release tag exists.
    let workspace = labcoat_core::workspace::discover(&project_root).map_err(core_error)?;
    let _local_harness =
        labcoat_core::compile::explicit_local_labcoat_test_override(&workspace.root)
            .map_err(core_error)?;

    let labcoat_bin = std::env::current_exe().map_err(io_error)?;
    let mut command = std::process::Command::new("cargo");
    labcoat_core::compile::patch_local_labcoat_crates(&mut command);
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
    labcoat_core::compile::patch_local_labcoat_crates(&mut command);
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

fn io_error(error: std::io::Error) -> EnvelopeError {
    EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: error.to_string(),
        hint: "check project permissions and available disk space",
    }
}
