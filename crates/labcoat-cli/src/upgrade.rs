//! `labcoat upgrade` — align a project's pins with the installed CLI.
//!
//! A Labcoat release is one compatibility unit: the CLI, its managed
//! runtime, the alkanes-rs revision contracts compile against, the
//! `labcoat-test` harness tag, and the Rust toolchain. Projects record those
//! pins when `labcoat init` creates them, so upgrading the CLI leaves older
//! projects compiling against the previous release. This module finds the
//! pins that differ and rewrites them in place, preserving the manifest's
//! formatting and comments.

use crate::contract::{CmdResult, EnvelopeError};
use serde::Serialize;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, Value};

const ALKANES_RS_REPO: &str = "kungfuflex/alkanes-rs";
const METASHREW_REPO: &str = "kungfuflex/metashrew";
const LABCOAT_REPO: &str = "jonatns/labcoat";
const WASM_TARGETS: [&str; 2] = ["wasm32-unknown-unknown", "wasm32-wasip1"];
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// One pin that differs from the installed CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinChange {
    pub file: String,
    pub item: String,
    pub from: Option<String>,
    pub to: String,
}

/// Expected pins for the installed CLI.
struct Expected {
    alkanes_rev: String,
    metashrew_tag: String,
    labcoat_tag: String,
    toolchain: String,
}

impl Expected {
    fn installed() -> Self {
        Self {
            alkanes_rev: labcoat_core::compile::ALKANES_RS_REV.to_string(),
            metashrew_tag: labcoat_core::compile::METASHREW_TAG.to_string(),
            labcoat_tag: format!("cli-v{}", env!("CARGO_PKG_VERSION")),
            toolchain: labcoat_core::compile::PROJECT_RUST_TOOLCHAIN.to_string(),
        }
    }
}

/// What `labcoat upgrade` would change in a project.
#[derive(Debug)]
pub struct PinReport {
    /// Labcoat-managed git dependencies found in the root manifest.
    pub managed: usize,
    pub changes: Vec<PinChange>,
}

/// Pins in the project at `root` that differ from the installed CLI.
pub fn plan(root: &Path) -> Result<PinReport, EnvelopeError> {
    let expected = Expected::installed();
    let mut manifest = read_manifest(root)?;
    let (managed, mut changes) = align_manifest(&mut manifest, &expected);
    changes.extend(toolchain_changes(root, &expected)?);
    Ok(PinReport { managed, changes })
}

/// `labcoat upgrade [--dry-run]`.
pub fn run(dry_run: bool) -> CmdResult {
    let cwd = std::env::current_dir().map_err(|e| io_error(Path::new("."), e))?;
    let root = crate::project::find_project_root(&cwd).ok_or_else(|| EnvelopeError {
        code: "CONFIG_INVALID",
        message: format!("{} is not inside a Labcoat project", cwd.display()),
        hint: "run `labcoat upgrade` from inside a project created by `labcoat init`",
    })?;
    upgrade(&root, dry_run)
}

fn upgrade(root: &Path, dry_run: bool) -> CmdResult {
    let expected = Expected::installed();
    let mut manifest = read_manifest(root)?;
    let (managed, mut changes) = align_manifest(&mut manifest, &expected);
    if managed == 0 {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: "Cargo.toml has no alkanes-rs or labcoat-test git dependencies to upgrade"
                .into(),
            hint: "compare Cargo.toml with a project from `labcoat init` and restore its [workspace.dependencies]",
        });
    }
    let toolchain = toolchain_changes(root, &expected)?;
    if !dry_run {
        if !changes.is_empty() {
            let path = root.join("Cargo.toml");
            std::fs::write(&path, manifest.to_string()).map_err(|e| io_error(&path, e))?;
        }
        if !toolchain.is_empty() {
            write_toolchain(root, &expected)?;
        }
    }
    changes.extend(toolchain);
    let up_to_date = changes.is_empty();
    Ok(serde_json::json!({
        "project": root,
        "labcoatVersion": env!("CARGO_PKG_VERSION"),
        "dryRun": dry_run,
        "upToDate": up_to_date,
        "changes": changes,
        "next": if up_to_date || dry_run {
            Vec::<&str>::new()
        } else {
            vec!["labcoat test", "commit Cargo.toml, Cargo.lock, and rust-toolchain.toml"]
        },
    }))
}

fn read_manifest(root: &Path) -> Result<DocumentMut, EnvelopeError> {
    let path = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| io_error(&path, e))?;
    text.parse::<DocumentMut>().map_err(|e| EnvelopeError {
        code: "CONFIG_INVALID",
        message: format!("{} is not valid TOML: {e}", path.display()),
        hint: "fix the project Cargo.toml and retry",
    })
}

/// Rewrite every Labcoat-managed git pin in the root manifest, returning how
/// many managed dependencies it found and what changed.
fn align_manifest(manifest: &mut DocumentMut, expected: &Expected) -> (usize, Vec<PinChange>) {
    let mut managed = 0;
    let mut changes = Vec::new();
    if let Some(table) = manifest
        .get_mut("workspace")
        .and_then(Item::as_table_like_mut)
        .and_then(|workspace| workspace.get_mut("dependencies"))
        .and_then(Item::as_table_mut)
    {
        align_table(
            table,
            "workspace.dependencies",
            expected,
            &mut managed,
            &mut changes,
        );
    }
    for name in DEPENDENCY_TABLES {
        if let Some(table) = manifest.get_mut(name).and_then(Item::as_table_mut) {
            align_table(table, name, expected, &mut managed, &mut changes);
        }
    }
    (managed, changes)
}

fn align_table(
    table: &mut Table,
    section: &str,
    expected: &Expected,
    managed: &mut usize,
    changes: &mut Vec<PinChange>,
) {
    for (name, item) in table.iter_mut() {
        let Some(dependency) = item.as_table_like_mut() else {
            continue;
        };
        let Some(git) = dependency
            .get("git")
            .and_then(Item::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let (key, value) = if git.contains(ALKANES_RS_REPO) {
            ("rev", &expected.alkanes_rev)
        } else if git.contains(METASHREW_REPO) {
            ("tag", &expected.metashrew_tag)
        } else if git.contains(LABCOAT_REPO) {
            ("tag", &expected.labcoat_tag)
        } else {
            continue;
        };
        *managed += 1;
        let current = ["rev", "tag", "branch"].into_iter().find_map(|field| {
            dependency
                .get(field)
                .and_then(Item::as_str)
                .map(str::to_owned)
        });
        let pinned_correctly = dependency.get(key).and_then(Item::as_str) == Some(value.as_str())
            && ["rev", "tag", "branch"]
                .into_iter()
                .filter(|field| *field != key)
                .all(|field| dependency.get(field).is_none());
        if pinned_correctly {
            continue;
        }
        for field in ["rev", "tag", "branch"] {
            if field != key {
                dependency.remove(field);
            }
        }
        dependency.insert(key, Item::Value(Value::from(value.as_str())));
        changes.push(PinChange {
            file: "Cargo.toml".into(),
            item: format!("{section}.{name}"),
            from: current,
            to: value.clone(),
        });
    }
}

fn toolchain_path(root: &Path) -> PathBuf {
    root.join("rust-toolchain.toml")
}

fn toolchain_changes(root: &Path, expected: &Expected) -> Result<Vec<PinChange>, EnvelopeError> {
    let path = toolchain_path(root);
    let change = |from: Option<String>| PinChange {
        file: "rust-toolchain.toml".into(),
        item: "toolchain.channel".into(),
        from,
        to: expected.toolchain.clone(),
    };
    if !path.is_file() {
        return Ok(vec![change(None)]);
    }
    let document = read_toolchain(&path)?;
    let toolchain = document.get("toolchain").and_then(Item::as_table_like);
    let channel = toolchain
        .and_then(|t| t.get("channel"))
        .and_then(Item::as_str)
        .map(str::to_owned);
    let targets: Vec<String> = toolchain
        .and_then(|t| t.get("targets"))
        .and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let has_targets = WASM_TARGETS
        .iter()
        .all(|t| targets.iter().any(|have| have == t));
    if channel.as_deref() == Some(expected.toolchain.as_str()) && has_targets {
        Ok(Vec::new())
    } else {
        Ok(vec![change(channel)])
    }
}

fn read_toolchain(path: &Path) -> Result<DocumentMut, EnvelopeError> {
    let text = std::fs::read_to_string(path).map_err(|e| io_error(path, e))?;
    text.parse::<DocumentMut>().map_err(|e| EnvelopeError {
        code: "CONFIG_INVALID",
        message: format!("{} is not valid TOML: {e}", path.display()),
        hint: "fix or delete rust-toolchain.toml and retry",
    })
}

/// Create `rust-toolchain.toml`, or set its channel and add any missing
/// WebAssembly targets while keeping other settings.
fn write_toolchain(root: &Path, expected: &Expected) -> Result<(), EnvelopeError> {
    let path = toolchain_path(root);
    let text = if path.is_file() {
        let mut document = read_toolchain(&path)?;
        let toolchain = document
            .entry("toolchain")
            .or_insert_with(|| Item::Table(Table::new()))
            .as_table_like_mut()
            .ok_or_else(|| EnvelopeError {
                code: "CONFIG_INVALID",
                message: format!("{} has a non-table `toolchain` key", path.display()),
                hint: "fix or delete rust-toolchain.toml and retry",
            })?;
        toolchain.insert(
            "channel",
            Item::Value(Value::from(expected.toolchain.as_str())),
        );
        let mut targets: Vec<String> = toolchain
            .get("targets")
            .and_then(Item::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        for target in WASM_TARGETS {
            if !targets.iter().any(|have| have == target) {
                targets.push(target.to_string());
            }
        }
        toolchain.insert(
            "targets",
            Item::Value(Value::Array(targets.iter().map(String::as_str).collect())),
        );
        document.to_string()
    } else {
        crate::project::TOOLCHAIN_TEMPLATE.replace("{{RUST_TOOLCHAIN}}", &expected.toolchain)
    };
    std::fs::write(&path, text).map_err(|e| io_error(&path, e))
}

fn io_error(path: &Path, error: std::io::Error) -> EnvelopeError {
    EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: format!("{}: {error}", path.display()),
        hint: "check project permissions",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_MANIFEST: &str = r#"[package]
name = "demo"

[workspace.dependencies]
# Keep these revisions synchronized with the Labcoat release and TOOLCHAIN.md.
alkanes-runtime = { git = "https://github.com/kungfuflex/alkanes-rs", rev = "714843c416e2ab57352a33f05b8461cf3f540f5a" }
alkanes-support = { git = "https://github.com/kungfuflex/alkanes-rs", branch = "main" }
metashrew-support = { git = "https://github.com/kungfuflex/metashrew", tag = "v9.0.5-rc.8" }
anyhow = "1.0"

[dev-dependencies]
labcoat-test = { git = "https://github.com/jonatns/labcoat", tag = "cli-v0.1.0" }
"#;

    fn temp_project(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("labcoat-upgrade-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("Cargo.toml"), OLD_MANIFEST).unwrap();
        root
    }

    #[test]
    fn upgrade_rewrites_stale_pins_and_keeps_formatting() {
        let root = temp_project("rewrite");
        let report = plan(&root).unwrap();
        assert_eq!(report.managed, 4);
        let items: Vec<_> = report.changes.iter().map(|c| c.item.as_str()).collect();
        assert_eq!(
            items,
            vec![
                "workspace.dependencies.alkanes-runtime",
                "workspace.dependencies.alkanes-support",
                "dev-dependencies.labcoat-test",
                "toolchain.channel",
            ]
        );
        // Planning never writes.
        assert_eq!(
            std::fs::read_to_string(root.join("Cargo.toml")).unwrap(),
            OLD_MANIFEST
        );

        upgrade(&root, false).unwrap();
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("# Keep these revisions synchronized"));
        assert!(manifest.contains("anyhow = \"1.0\""));
        assert_eq!(
            manifest
                .matches(labcoat_core::compile::ALKANES_RS_REV)
                .count(),
            2
        );
        assert!(!manifest.contains("branch"));
        assert!(manifest.contains(&format!("tag = \"cli-v{}\"", env!("CARGO_PKG_VERSION"))));
        let toolchain = std::fs::read_to_string(root.join("rust-toolchain.toml")).unwrap();
        assert!(toolchain.contains(&format!(
            "channel = \"{}\"",
            labcoat_core::compile::PROJECT_RUST_TOOLCHAIN
        )));
        assert!(plan(&root).unwrap().changes.is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn upgrade_keeps_custom_toolchain_settings() {
        let root = temp_project("toolchain");
        std::fs::write(
            root.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.88.0\"\nprofile = \"minimal\"\ntargets = [\"wasm32-unknown-unknown\"]\n",
        )
        .unwrap();
        upgrade(&root, false).unwrap();
        let toolchain = std::fs::read_to_string(root.join("rust-toolchain.toml")).unwrap();
        assert!(toolchain.contains("profile = \"minimal\""));
        assert!(toolchain.contains("wasm32-wasip1"));
        assert!(toolchain.contains(labcoat_core::compile::PROJECT_RUST_TOOLCHAIN));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_manifest_without_managed_pins_is_an_error() {
        let root = temp_project("empty");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        assert_eq!(plan(&root).unwrap().managed, 0);
        assert_eq!(upgrade(&root, false).unwrap_err().code, "CONFIG_INVALID");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let root = temp_project("dry");
        let result = upgrade(&root, true).unwrap();
        assert_eq!(result["upToDate"], false);
        assert_eq!(
            std::fs::read_to_string(root.join("Cargo.toml")).unwrap(),
            OLD_MANIFEST
        );
        assert!(!root.join("rust-toolchain.toml").exists());
        std::fs::remove_dir_all(root).ok();
    }
}
