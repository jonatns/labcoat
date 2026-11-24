use crate::contract::{CmdResult, Ctx, EnvelopeError};

/// Stops the e2e network when the run ends, however it ends. Nothing else
/// knows the port it runs on, so nothing else could stop it later.
struct RunningNetwork(isomer_core::LabcoatNetwork);

impl Drop for RunningNetwork {
    fn drop(&mut self) {
        let _ = self.0.stop();
    }
}

/// `labcoat test --e2e [filter]` — on the project's own e2e network (see
/// `e2e_instance`): reset its chain (opt-out with `--no-reset`), run the
/// `deploy.rs` deployment when present, then run the project's ignored
/// `tests/e2e.rs` tests with the context env set. The Labcoat Network the
/// developer works on, and its deployment records, are not touched.
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
    let project_root = ctx.config.project_root();
    let e2e_target = project_root.join("tests/e2e.rs");
    if !e2e_target.is_file() {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: "no e2e test target at tests/e2e.rs".into(),
            hint: "create tests/e2e.rs with #[ignore] tests using labcoat_test::e2e::E2e",
        });
    }
    let instance = crate::e2e_instance::current().ok_or_else(|| EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: "the e2e network was not prepared for this run".into(),
        hint: "run e2e tests with `labcoat test --e2e`",
    })?;
    if ctx.config.rpc_url != instance.rpc_url {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: format!(
                "e2e tests run on their own network at {}, not {}",
                instance.rpc_url, ctx.config.rpc_url
            ),
            hint: "drop the --rpc-url override",
        });
    }

    // Honor LABCOAT_TEST_CRATE_PATH like `labcoat test` does, so CI and
    // packagers can run e2e tests before the release tag exists.
    let workspace = labcoat_core::workspace::discover(&project_root).map_err(core_error)?;
    let _local_harness =
        labcoat_core::compile::explicit_local_labcoat_test_override(&workspace.root)
            .map_err(core_error)?;

    // Build the tests and count the selected ones before a chain exists: a
    // compile error or a filter that matches nothing should cost nothing.
    let selected = selected_e2e_tests(&project_root, filter)?;
    if selected == 0 {
        return Err(EnvelopeError {
            code: "TEST_FAILED",
            message: match filter {
                Some(filter) => format!("no e2e test in tests/e2e.rs matches `{filter}`"),
                None => "tests/e2e.rs has no #[ignore] tests to run".into(),
            },
            hint: "check the filter against the test names in tests/e2e.rs; e2e tests must be marked #[ignore]",
        });
    }

    let mut network = RunningNetwork(isomer_core::LabcoatNetwork::new());
    network
        .0
        .ensure_binaries(|_, _| {})
        .await
        .map_err(|message| EnvelopeError {
            code: "TOOLKIT_ERROR",
            message,
            hint: "check network access to the binary hosts",
        })?;
    if !no_reset {
        network.0.reset().map_err(|message| EnvelopeError {
            code: "TOOLKIT_ERROR",
            message,
            hint: "run `labcoat doctor`",
        })?;
        // A fresh chain starts with no deployments recorded.
        std::fs::remove_dir_all(&instance.records_dir)
            .and_then(|()| std::fs::create_dir_all(&instance.records_dir))
            .map_err(io_error)?;
    }
    let node_log = isomer_core::config::get_logs_dir().join("node.log");
    network.0.start().map_err(|message| EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: format!("{message} (e2e network log: {})", node_log.display()),
        hint: "run `labcoat doctor`",
    })?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !network.0.status().await.is_ready {
        if std::time::Instant::now() > deadline {
            return Err(EnvelopeError {
                code: "RPC_UNREACHABLE",
                message: format!(
                    "the e2e network was not ready after 120s (log: {})",
                    node_log.display()
                ),
                hint: "run `labcoat doctor`",
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
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
        network
            .0
            .fund(&address, 2.0)
            .await
            .map_err(|message| EnvelopeError {
                code: "TOOLKIT_ERROR",
                message,
                hint:
                    "re-run `labcoat test --e2e`; without --no-reset it starts from a fresh chain",
            })?;
        network
            .0
            .mine(1, None)
            .await
            .map_err(|message| EnvelopeError {
                code: "TOOLKIT_ERROR",
                message,
                hint:
                    "re-run `labcoat test --e2e`; without --no-reset it starts from a fresh chain",
            })?;
    }

    let deployed = if project_root
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
        Some(serde_json::to_value(report).expect("serializable deploy report"))
    } else {
        None
    };

    let labcoat_bin = std::env::current_exe().map_err(io_error)?;
    let mut command = e2e_cargo_test(&project_root, filter);
    let output = command
        .env("LABCOAT_E2E_BIN", &labcoat_bin)
        .env("LABCOAT_E2E_ROOT", &project_root)
        .output()
        .map_err(|e| {
            core_error(labcoat_core::LabcoatError::cargo_unavailable(
                &e,
                "run the e2e tests",
            ))
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
    drop(network);

    Ok(serde_json::json!({
        "e2e": true,
        "reset": !no_reset,
        "deployed": deployed,
        "passed": true,
        "tests": selected,
        "network": {
            "rpcUrl": instance.rpc_url,
            "dataHome": instance.data_home,
            "recordsDir": instance.records_dir,
        },
        "output": String::from_utf8_lossy(&output.stdout),
    }))
}

/// `cargo test` for the project's ignored e2e tests, narrowed by `filter`.
fn e2e_cargo_test(project_root: &std::path::Path, filter: Option<&str>) -> std::process::Command {
    let mut command = std::process::Command::new("cargo");
    labcoat_core::compile::patch_local_labcoat_crates(&mut command);
    command.args(["test", "--test", "e2e", "--", "--ignored"]);
    if let Some(filter) = filter {
        command.arg(filter);
    }
    command.current_dir(project_root);
    command
}

/// Build the e2e tests and count the ones a run with `filter` would execute.
fn selected_e2e_tests(
    project_root: &std::path::Path,
    filter: Option<&str>,
) -> Result<usize, EnvelopeError> {
    let output = e2e_cargo_test(project_root, filter)
        .arg("--list")
        .output()
        .map_err(|e| {
            core_error(labcoat_core::LabcoatError::cargo_unavailable(
                &e,
                "build the e2e tests",
            ))
        })?;
    if !output.status.success() {
        return Err(EnvelopeError {
            code: "TEST_FAILED",
            message: format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            hint: "fix the build errors in tests/e2e.rs and re-run `labcoat test --e2e`",
        });
    }
    Ok(count_listed_tests(&String::from_utf8_lossy(&output.stdout)))
}

/// The tests in libtest's `--list` output, which names one per line as
/// `<name>: test`.
fn count_listed_tests(listing: &str) -> usize {
    listing
        .lines()
        .filter(|line| line.trim_end().ends_with(": test"))
        .count()
}

pub fn run(package: Option<&str>) -> CmdResult {
    let project_root = crate::project::root();
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
        .map_err(|e| {
            core_error(labcoat_core::LabcoatError::cargo_unavailable(
                &e,
                "run the tests",
            ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_tests_libtest_lists() {
        let listing = "increments_on_chain: test\nproxy::upgrades: test\n\n2 tests, 0 benchmarks\n";
        assert_eq!(count_listed_tests(listing), 2);
        // A filter that matches nothing lists nothing.
        assert_eq!(count_listed_tests("\n0 tests, 0 benchmarks\n"), 0);
        assert_eq!(count_listed_tests("some_bench: benchmark\n"), 0);
    }

    #[test]
    fn the_listing_and_the_run_select_the_same_tests() {
        let root = std::path::Path::new("/project");
        let run = e2e_cargo_test(root, Some("increments"));
        let args: Vec<_> = run.get_args().map(|arg| arg.to_string_lossy()).collect();
        assert!(args.ends_with(&[
            "test".into(),
            "--test".into(),
            "e2e".into(),
            "--".into(),
            "--ignored".into(),
            "increments".into()
        ]));
        assert_eq!(run.get_current_dir(), Some(root));
    }
}
