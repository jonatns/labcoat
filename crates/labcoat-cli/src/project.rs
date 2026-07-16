use crate::contract::{CmdResult, EnvelopeError};
use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        include_str!("../templates/default/Cargo.toml"),
    ),
    (
        "src/lib.rs",
        include_str!("../templates/default/src/lib.rs"),
    ),
    (
        "contracts/example/Cargo.toml",
        include_str!("../templates/default/contracts/example/Cargo.toml"),
    ),
    (
        "contracts/example/src/lib.rs",
        include_str!("../templates/default/contracts/example/src/lib.rs"),
    ),
    (
        "tests/example.rs",
        include_str!("../templates/default/tests/example.rs"),
    ),
    (
        "labcoat.toml",
        include_str!("../templates/default/labcoat.toml"),
    ),
    (".gitignore", include_str!("../templates/default/gitignore")),
    ("AGENTS.md", include_str!("../templates/default/AGENTS.md")),
    ("SKILL.md", include_str!("../templates/default/SKILL.md")),
];

const CONTRACT_MANIFEST: &str = include_str!("../templates/contract/Cargo.toml");
const CONTRACT_SOURCE: &str = include_str!("../templates/contract/src/lib.rs");
const CONTRACT_TEST: &str = include_str!("../templates/contract/test.rs");

pub fn init(directory: Option<&str>, force: bool, contract: Option<&str>) -> CmdResult {
    let target = directory
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if let Some(name) = contract {
        validate_contract_name(name)?;
        ensure_contract_destinations_available(&target, name)?;
    }

    if target.exists() && !force && !is_empty(&target)? {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: format!(
                "refusing to scaffold into non-empty directory {}",
                target.display()
            ),
            hint: "choose an empty directory or pass --force to overlay the template",
        });
    }

    std::fs::create_dir_all(&target).map_err(|e| io_error(&target, e))?;
    for (relative, contents) in FILES {
        if contract.is_some()
            && (*relative == "contracts/example/Cargo.toml"
                || *relative == "contracts/example/src/lib.rs"
                || *relative == "tests/example.rs")
        {
            continue;
        }
        let destination = target.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        let rendered = contents.replace("{{LABCOAT_VERSION}}", env!("CARGO_PKG_VERSION"));
        std::fs::write(&destination, rendered).map_err(|e| io_error(&destination, e))?;
    }

    let generated = if let Some(name) = contract {
        scaffold_contract(&target, name)?
    } else {
        Vec::new()
    };

    let initial_contract = contract.unwrap_or("example");

    Ok(serde_json::json!({
        "directory": target,
        "template": "default",
        "contract": initial_contract,
        "files": FILES.iter().map(|(path, _)| *path).filter(|path| {
            contract.is_none() || !path.starts_with("contracts/example/") && *path != "tests/example.rs"
        }).chain(generated.iter().map(String::as_str)).collect::<Vec<_>>(),
        "next": ["labcoat test", "labcoat up", "labcoat wallet init"]
    }))
}

pub fn new_contract(name: &str) -> CmdResult {
    let root = Path::new(".");
    if !root.join("labcoat.toml").is_file() || !root.join("Cargo.toml").is_file() {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: "current directory is not a Labcoat project".into(),
            hint: "run this command from a project created by `labcoat init`",
        });
    }
    let files = scaffold_contract(root, name)?;
    Ok(serde_json::json!({ "contract": name, "files": files }))
}

fn scaffold_contract(root: &Path, name: &str) -> Result<Vec<String>, EnvelopeError> {
    validate_contract_name(name)?;
    ensure_contract_destinations_available(root, name)?;
    let rust_name = rust_type_name(name);
    let relative = contract_paths(name);
    let templates = [CONTRACT_MANIFEST, CONTRACT_SOURCE, CONTRACT_TEST];
    for (path, template) in relative.iter().zip(templates) {
        let destination = root.join(path);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        let rendered = template
            .replace("{{CONTRACT_NAME}}", name)
            .replace("{{CONTRACT_RUST_NAME}}", &rust_name);
        std::fs::write(&destination, rendered).map_err(|e| io_error(&destination, e))?;
    }
    Ok(relative.into_iter().collect())
}

fn contract_paths(name: &str) -> [String; 3] {
    [
        format!("contracts/{name}/Cargo.toml"),
        format!("contracts/{name}/src/lib.rs"),
        format!("tests/{name}.rs"),
    ]
}

fn ensure_contract_destinations_available(root: &Path, name: &str) -> Result<(), EnvelopeError> {
    for path in contract_paths(name) {
        if root.join(&path).exists() {
            return Err(EnvelopeError {
                code: "CONFIG_INVALID",
                message: format!("refusing to overwrite {}", root.join(path).display()),
                hint: "choose another contract name or remove the existing files",
            });
        }
    }
    Ok(())
}

fn validate_contract_name(name: &str) -> Result<(), EnvelopeError> {
    let valid = !name.is_empty()
        && name.split('-').all(|part| {
            !part.is_empty()
                && part.as_bytes()[0].is_ascii_lowercase()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        });
    if valid {
        Ok(())
    } else {
        Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: format!("invalid contract name `{name}`"),
            hint: "use kebab-case beginning with a lowercase letter, for example `ens-registry`",
        })
    }
}

fn rust_type_name(name: &str) -> String {
    let mut result = String::new();
    for part in name.split('-') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.extend(chars);
        }
    }
    result
}

fn is_empty(path: &Path) -> Result<bool, EnvelopeError> {
    if !path.is_dir() {
        return Ok(false);
    }
    let mut entries = std::fs::read_dir(path).map_err(|e| io_error(path, e))?;
    Ok(entries.next().is_none())
}

fn io_error(path: &Path, error: std::io::Error) -> EnvelopeError {
    EnvelopeError {
        code: "TOOLKIT_ERROR",
        message: format!("cannot write {}: {}", path.display(), error),
        hint: "check the destination path and permissions",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffolds_and_refuses_non_empty_directories() {
        let root = std::env::temp_dir().join(format!("labcoat-init-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let result = init(root.to_str(), false, None).unwrap();
        assert_eq!(result["template"], "default");
        assert!(root.join("labcoat.toml").exists());
        assert!(root.join("contracts/example/Cargo.toml").exists());
        assert!(root.join("contracts/example/src/lib.rs").exists());
        assert!(!root.join("crates").exists());
        assert!(root.join("tests/example.rs").exists());
        assert!(std::fs::read_to_string(root.join("tests/example.rs"))
            .unwrap()
            .contains("for_contract(\"example\")"));
        assert!(std::fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains(&format!(
                "labcoat-test = \"={}\"",
                env!("CARGO_PKG_VERSION")
            )));
        assert!(init(root.to_str(), false, None).is_err());
        assert!(init(root.to_str(), true, None).is_ok());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn scaffolds_a_named_initial_contract() {
        let root = std::env::temp_dir().join(format!("labcoat-named-init-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let result = init(root.to_str(), false, Some("ens-registry")).unwrap();
        assert_eq!(result["contract"], "ens-registry");
        assert!(root.join("contracts/ens-registry/Cargo.toml").exists());
        assert!(root.join("tests/ens-registry.rs").exists());
        assert!(!root.join("contracts/example").exists());
        let source =
            std::fs::read_to_string(root.join("contracts/ens-registry/src/lib.rs")).unwrap();
        assert!(source.contains("pub struct EnsRegistry"));
        assert!(source.contains("enum EnsRegistryMessage"));
        assert!(!source.contains("EnsRegistryContract"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn adds_contracts_without_overwriting_or_partial_collision_writes() {
        let root =
            std::env::temp_dir().join(format!("labcoat-contract-new-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        init(root.to_str(), false, None).unwrap();
        let files = scaffold_contract(&root, "name-registry").unwrap();
        assert_eq!(files.len(), 3);
        assert!(root.join("contracts/example/Cargo.toml").exists());
        assert!(root.join("contracts/name-registry/Cargo.toml").exists());
        assert!(scaffold_contract(&root, "name-registry").is_err());

        std::fs::write(root.join("tests/collision.rs"), "existing").unwrap();
        assert!(scaffold_contract(&root, "collision").is_err());
        assert!(!root.join("contracts/collision").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("tests/collision.rs")).unwrap(),
            "existing"
        );
        assert!(scaffold_contract(&root, "Bad_Name").is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn template_pins_match_the_core_toolchain() {
        let manifest = include_str!("../templates/default/Cargo.toml");
        assert!(manifest.contains(labcoat_core::compile::ALKANES_RS_REV));
        assert!(manifest.contains(
            "metashrew-support = { git = \"https://github.com/kungfuflex/metashrew\", branch = \"develop\" }"
        ));
        assert!(!manifest.contains("sandshrewmetaprotocols/metashrew"));
        assert!(manifest.contains("serde_with = { version = \"=3.16.1\""));
        assert!(manifest.contains("time = { version = \"=0.3.44\""));
        let contract = include_str!("../templates/default/contracts/example/Cargo.toml");
        assert!(contract.contains("serde_with.workspace = true"));
        assert!(contract.contains("time.workspace = true"));
        let workspace_manifest = include_str!("../../../Cargo.toml");
        assert!(workspace_manifest.contains("wasmi = \"=0.37.2\""));
    }
}
