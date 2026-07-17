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
        "contracts/counter/Cargo.toml",
        include_str!("../templates/default/contracts/counter/Cargo.toml"),
    ),
    (
        "contracts/counter/src/lib.rs",
        include_str!("../templates/default/contracts/counter/src/lib.rs"),
    ),
    (
        "tests/counter.rs",
        include_str!("../templates/default/tests/counter.rs"),
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

pub fn init(directory: Option<&str>, force: bool) -> CmdResult {
    let target = directory
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

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
        let destination = target.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        let rendered = contents.replace("{{LABCOAT_VERSION}}", env!("CARGO_PKG_VERSION"));
        std::fs::write(&destination, rendered).map_err(|e| io_error(&destination, e))?;
    }

    Ok(serde_json::json!({
        "directory": target,
        "template": "default",
        "contract": "counter",
        "files": FILES.iter().map(|(path, _)| *path).collect::<Vec<_>>(),
        "next": ["labcoat test", "labcoat up", "labcoat wallet init"]
    }))
}

pub fn new_contract(name: &str) -> CmdResult {
    let cwd = std::env::current_dir().map_err(|e| io_error(Path::new("."), e))?;
    new_contract_from(&cwd, name)
}

fn new_contract_from(start: &Path, name: &str) -> CmdResult {
    let root = find_project_root(start).ok_or_else(|| EnvelopeError {
        code: "CONFIG_INVALID",
        message: format!("{} is not inside a Labcoat project", start.display()),
        hint: "run `labcoat init` to create a project, then retry from inside it",
    })?;
    let files = scaffold_contract(&root, name)?;
    Ok(serde_json::json!({ "contract": name, "files": files }))
}

fn find_project_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|candidate| {
        (candidate.join("labcoat.toml").is_file() && candidate.join("Cargo.toml").is_file())
            .then(|| candidate.to_path_buf())
    })
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
        let result = init(root.to_str(), false).unwrap();
        assert_eq!(result["template"], "default");
        assert_eq!(result["contract"], "counter");
        assert!(root.join("labcoat.toml").exists());
        assert!(root.join("contracts/counter/Cargo.toml").exists());
        assert!(root.join("contracts/counter/src/lib.rs").exists());
        assert!(!root.join("crates").exists());
        let workspace_manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(workspace_manifest.contains("members = [\"contracts/*\"]"));
        assert!(!workspace_manifest.contains("crates/*"));
        assert!(root.join("tests/counter.rs").exists());
        assert!(std::fs::read_to_string(root.join("tests/counter.rs"))
            .unwrap()
            .contains("for_contract(\"counter\")"));
        assert!(std::fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains(&format!(
                "labcoat-test = \"={}\"",
                env!("CARGO_PKG_VERSION")
            )));
        assert!(init(root.to_str(), false).is_err());
        std::fs::write(
            root.join("contracts/counter/src/lib.rs"),
            "overwritten by --force",
        )
        .unwrap();
        assert!(init(root.to_str(), true).is_ok());
        assert!(
            std::fs::read_to_string(root.join("contracts/counter/src/lib.rs"))
                .unwrap()
                .contains("pub struct Counter")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn adds_contracts_without_overwriting_or_partial_collision_writes() {
        let root =
            std::env::temp_dir().join(format!("labcoat-contract-new-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        init(root.to_str(), false).unwrap();
        let nested = root.join("contracts/counter/src");
        let result = new_contract_from(&nested, "name-registry").unwrap();
        let files = result["files"].as_array().unwrap();
        assert_eq!(files.len(), 3);
        assert!(root.join("contracts/counter/Cargo.toml").exists());
        assert!(root.join("contracts/name-registry/Cargo.toml").exists());
        assert!(new_contract_from(&nested, "name-registry").is_err());

        std::fs::write(root.join("tests/collision.rs"), "existing").unwrap();
        assert!(new_contract_from(&nested, "collision").is_err());
        assert!(!root.join("contracts/collision").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("tests/collision.rs")).unwrap(),
            "existing"
        );
        assert!(new_contract_from(&nested, "Bad_Name").is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn new_contract_rejects_locations_outside_a_labcoat_project() {
        let root =
            std::env::temp_dir().join(format!("labcoat-not-a-project-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        let error = new_contract_from(&root, "example").unwrap_err();
        assert_eq!(error.code, "CONFIG_INVALID");
        assert!(error.hint.contains("labcoat init"));
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
        let contract = include_str!("../templates/default/contracts/counter/Cargo.toml");
        assert!(contract.contains("serde_with.workspace = true"));
        assert!(contract.contains("time.workspace = true"));
        let workspace_manifest = include_str!("../../../Cargo.toml");
        assert!(workspace_manifest.contains("wasmi = \"=0.37.2\""));
    }
}
