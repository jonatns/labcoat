//! Runs a project's deployment script (`deploy.rs`, the `deploy` example
//! target) and reads the graph it declares.
//!
//! The script is ordinary host-side Rust, compiled and run by Cargo with no
//! access to a signer: it only writes the deployment graph, which
//! `labcoat_core::deployment` validates before anything is planned.

use labcoat_core::deployment::{self, Graph};
use labcoat_core::LabcoatError;
use std::path::Path;

fn script_error(message: String, hint: &'static str) -> LabcoatError {
    LabcoatError::new("DEPLOY_SCRIPT_INVALID", message, hint)
}

/// Compile and run the project's deployment script for `network`.
pub fn load_graph(root: &Path, network: &str) -> Result<Graph, LabcoatError> {
    let script = root.join(deployment::SCRIPT);
    if !script.is_file() {
        return Err(script_error(
            format!("no deployment script at {}", script.display()),
            "create deploy.rs and register it in Cargo.toml as `[[example]] name = \"deploy\" path = \"deploy.rs\"` — `labcoat init` scaffolds both",
        ));
    }
    let workspace = labcoat_core::workspace::discover(root)?;
    // Honor LABCOAT_TEST_CRATE_PATH like `labcoat test` does.
    let _local_harness =
        labcoat_core::compile::explicit_local_labcoat_test_override(&workspace.root)?;

    let scratch = root.join(".labcoat");
    std::fs::create_dir_all(&scratch).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("cannot create {}: {e}", scratch.display()),
            "check project permissions",
        )
    })?;
    let graph_path = scratch.join(format!("deploy-graph-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&graph_path);

    let mut command = std::process::Command::new("cargo");
    labcoat_core::compile::patch_local_labcoat_crates(&mut command);
    let output = command
        .args(["run", "--quiet", "--example", deployment::SCRIPT_TARGET])
        .current_dir(root)
        .env("LABCOAT_DEPLOY_NETWORK", network)
        .env("LABCOAT_DEPLOY_GRAPH", &graph_path)
        .output()
        .map_err(|e| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("failed to run cargo: {e}"),
                "install Cargo and run `labcoat doctor`",
            )
        })?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&graph_path);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no example target named") {
            return Err(script_error(
                "Cargo.toml does not register deploy.rs as the `deploy` example target".into(),
                "add `[[example]]` with `name = \"deploy\"` and `path = \"deploy.rs\"` to the project's Cargo.toml",
            ));
        }
        return Err(script_error(
            format!("deploy.rs failed to build or run:\n{}", stderr.trim_end()),
            "fix deploy.rs — `cargo run --example deploy` reproduces this outside labcoat",
        ));
    }
    let text = std::fs::read_to_string(&graph_path).map_err(|e| {
        script_error(
            format!("deploy.rs ran but wrote no deployment graph: {e}"),
            "call `labcoat::run(|d| { ... })` from deploy.rs's `main`",
        )
    })?;
    let _ = std::fs::remove_file(&graph_path);
    deployment::parse(&text, network)
}
