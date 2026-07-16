//! Cargo workspace discovery for contract packages.

use crate::error::{LabcoatError, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractPackage {
    pub name: String,
    pub manifest_dir: PathBuf,
    pub lib_src_path: PathBuf,
    /// Cargo's actual library target name (already normalized for artifacts).
    pub lib_target_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTestTarget {
    pub name: String,
    pub src_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub root: PathBuf,
    pub target_directory: PathBuf,
    pub contracts: Vec<ContractPackage>,
    pub host_test_targets: Vec<HostTestTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    workspace_root: PathBuf,
    target_directory: PathBuf,
    workspace_members: Vec<String>,
    packages: Vec<MetadataPackage>,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
    targets: Vec<MetadataTarget>,
}

#[derive(Debug, Deserialize)]
struct MetadataTarget {
    name: String,
    kind: Vec<String>,
    crate_types: Vec<String>,
    src_path: PathBuf,
}

pub fn discover(cwd: &Path) -> Result<WorkspaceInfo> {
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(cwd)
        .output()
        .map_err(|e| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!("failed to run cargo metadata: {e}"),
                "run Labcoat from a Cargo workspace",
            )
        })?;
    if !output.status.success() {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!(
                "cargo metadata failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            ),
            "fix the workspace Cargo.toml and retry",
        ));
    }
    classify_json(&output.stdout)
}

fn classify_json(json: &[u8]) -> Result<WorkspaceInfo> {
    let metadata: CargoMetadata = serde_json::from_slice(json).map_err(|e| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("cargo metadata returned invalid JSON: {e}"),
            "re-run with RUST_LOG=debug",
        )
    })?;
    classify(metadata)
}

fn classify(metadata: CargoMetadata) -> Result<WorkspaceInfo> {
    let members: HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let contracts_dir = metadata.workspace_root.join("contracts");
    let root_manifest = metadata.workspace_root.join("Cargo.toml");
    let mut contracts = Vec::new();
    let mut host_test_targets = Vec::new();

    for package in metadata
        .packages
        .iter()
        .filter(|package| members.contains(package.id.as_str()))
    {
        let under_contracts = package.manifest_path.starts_with(&contracts_dir);
        let cdylib = package
            .targets
            .iter()
            .find(|target| target.crate_types.iter().any(|kind| kind == "cdylib"));

        if under_contracts {
            let target = cdylib.ok_or_else(|| {
                LabcoatError::new(
                    "CONFIG_INVALID",
                    format!(
                        "contract package `{}` has no cdylib library target",
                        package.name
                    ),
                    "add `[lib] crate-type = [\"cdylib\", \"rlib\"]` to the contract Cargo.toml",
                )
            })?;
            contracts.push(ContractPackage {
                name: package.name.clone(),
                manifest_dir: package
                    .manifest_path
                    .parent()
                    .unwrap_or(&metadata.workspace_root)
                    .to_path_buf(),
                lib_src_path: target.src_path.clone(),
                lib_target_name: target.name.clone(),
            });
        } else if cdylib.is_some() {
            tracing::warn!(
                package = %package.name,
                "ignoring cdylib workspace package outside contracts/"
            );
        }

        if package.manifest_path == root_manifest {
            host_test_targets.extend(
                package
                    .targets
                    .iter()
                    .filter(|target| target.kind.iter().any(|kind| kind == "test"))
                    .map(|target| HostTestTarget {
                        name: target.name.clone(),
                        src_path: target.src_path.clone(),
                    }),
            );
        }
    }

    contracts.sort_by(|a, b| a.name.cmp(&b.name));
    host_test_targets.sort_by(|a, b| a.name.cmp(&b.name));
    if contracts.is_empty() {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            "no Cargo contract packages found under contracts/",
            "add a contracts/<name>/Cargo.toml with a cdylib library target",
        ));
    }
    for pair in contracts.windows(2) {
        if pair[0].name == pair[1].name {
            return Err(LabcoatError::new(
                "CONFIG_INVALID",
                format!("duplicate contract package name `{}`", pair[0].name),
                "give every contract package a unique Cargo package name",
            ));
        }
    }

    Ok(WorkspaceInfo {
        root: metadata.workspace_root,
        target_directory: metadata.target_directory,
        contracts,
        host_test_targets,
    })
}

pub fn select(workspace: &WorkspaceInfo, package: Option<&str>) -> Result<Vec<ContractPackage>> {
    let Some(package) = package else {
        return Ok(workspace.contracts.clone());
    };
    if Path::new(package).extension().and_then(|ext| ext.to_str()) == Some("rs") {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            "loose .rs contracts are no longer supported",
            "migrate the contract to contracts/<name>/Cargo.toml (see docs/MIGRATING.md)",
        ));
    }
    workspace
        .contracts
        .iter()
        .find(|contract| contract.name == package)
        .cloned()
        .map(|contract| vec![contract])
        .ok_or_else(|| {
            LabcoatError::new(
                "PACKAGE_NOT_FOUND",
                format!(
                    "contract package `{package}` was not found (available: {})",
                    workspace
                        .contracts
                        .iter()
                        .map(|contract| contract.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                "pass a discovered Cargo package name",
            )
        })
}

pub fn host_test_for_package<'a>(
    workspace: &'a WorkspaceInfo,
    package: &str,
) -> Option<&'a HostTestTarget> {
    let expected = format!("{package}.rs");
    workspace.host_test_targets.iter().find(|target| {
        target.src_path.file_name().and_then(|name| name.to_str()) == Some(expected.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(packages: &str, members: &str) -> Vec<u8> {
        format!(
            r#"{{"workspace_root":"/p","target_directory":"/p/target","workspace_members":[{members}],"packages":[{packages}]}}"#
        )
        .into_bytes()
    }

    fn package(id: &str, name: &str, path: &str, crate_types: &str, target: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"{name}","manifest_path":"{path}/Cargo.toml","targets":[{{"name":"{target}","kind":["lib"],"crate_types":[{crate_types}],"src_path":"{path}/src/lib.rs"}}]}}"#
        )
    }

    #[test]
    fn classifies_and_sorts_contracts() {
        let b = package("b-id", "b", "/p/contracts/b", "\"cdylib\",\"rlib\"", "b");
        let a = package("a-id", "a-token", "/p/contracts/a", "\"cdylib\"", "a_token");
        let json = metadata(&format!("{b},{a}"), "\"a-id\",\"b-id\"");
        let ws = classify_json(&json).unwrap();
        assert_eq!(ws.contracts[0].name, "a-token");
        assert_eq!(ws.contracts[0].lib_target_name, "a_token");
        assert_eq!(ws.contracts[1].name, "b");
    }

    #[test]
    fn rejects_contract_without_cdylib() {
        let p = package("a-id", "a", "/p/contracts/a", "\"rlib\"", "a");
        let err = classify_json(&metadata(&p, "\"a-id\"")).unwrap_err();
        assert_eq!(err.code, "CONFIG_INVALID");
        assert!(err.message.contains("cdylib"));
    }

    #[test]
    fn rejects_unknown_and_loose_file_selectors() {
        let p = package("a-id", "a", "/p/contracts/a", "\"cdylib\"", "a");
        let ws = classify_json(&metadata(&p, "\"a-id\"")).unwrap();
        assert_eq!(
            select(&ws, Some("nope")).unwrap_err().code,
            "PACKAGE_NOT_FOUND"
        );
        assert_eq!(
            select(&ws, Some("contracts/A.rs")).unwrap_err().code,
            "CONFIG_INVALID"
        );
    }
}
