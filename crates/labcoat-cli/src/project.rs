use crate::contract::{CmdResult, EnvelopeError};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        include_str!("../templates/default/Cargo.toml.tmpl"),
    ),
    (
        "src/lib.rs",
        include_str!("../templates/default/src/lib.rs"),
    ),
    (
        "contracts/counter/Cargo.toml",
        include_str!("../templates/default/contracts/counter/Cargo.toml.tmpl"),
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
        "tests/e2e.rs",
        include_str!("../templates/default/tests/e2e.rs"),
    ),
    (
        "alkanes.hcl",
        include_str!("../templates/default/alkanes.hcl"),
    ),
    (
        "labcoat.toml",
        include_str!("../templates/default/labcoat.toml"),
    ),
    ("rust-toolchain.toml", TOOLCHAIN_TEMPLATE),
    (".gitignore", include_str!("../templates/default/gitignore")),
    ("AGENTS.md", include_str!("../templates/default/AGENTS.md")),
    ("SKILL.md", include_str!("../templates/default/SKILL.md")),
];

/// Template for a project's `rust-toolchain.toml` (`{{RUST_TOOLCHAIN}}`).
pub(crate) const TOOLCHAIN_TEMPLATE: &str =
    include_str!("../templates/default/rust-toolchain.toml");

const CONTRACT_MANIFEST: &str = include_str!("../templates/contract/Cargo.toml.tmpl");
const CONTRACT_SOURCE: &str = include_str!("../templates/contract/src/lib.rs");
const CONTRACT_TEST: &str = include_str!("../templates/contract/test.rs");

pub fn init(project_name: &str) -> CmdResult {
    let parent = std::env::current_dir().map_err(|error| io_error(Path::new("."), error))?;
    init_in(&parent, project_name)
}

fn init_in(parent: &Path, project_name: &str) -> CmdResult {
    validate_name(project_name, "project")?;
    let target = parent.join(project_name);

    if target.exists() {
        return Err(EnvelopeError {
            code: "CONFIG_INVALID",
            message: format!(
                "refusing to scaffold into existing path {}",
                target.display()
            ),
            hint: "choose a new project name",
        });
    }

    std::fs::create_dir_all(&target).map_err(|e| io_error(&target, e))?;
    for (relative, contents) in FILES {
        let destination = target.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        let rendered = contents
            .replace("{{LABCOAT_VERSION}}", env!("CARGO_PKG_VERSION"))
            .replace("{{PROJECT_NAME}}", project_name)
            .replace(
                "{{RUST_TOOLCHAIN}}",
                labcoat_core::compile::PROJECT_RUST_TOOLCHAIN,
            );
        std::fs::write(&destination, rendered).map_err(|e| io_error(&destination, e))?;
    }

    Ok(serde_json::json!({
        "project": project_name,
        "directory": target,
        "template": "default",
        "contract": "counter",
        "files": FILES.iter().map(|(path, _)| *path).collect::<Vec<_>>(),
        "next": ["labcoat test", "labcoat up", "labcoat wallet init"]
    }))
}

pub fn missing_project_name() -> EnvelopeError {
    EnvelopeError {
        code: "CONFIG_INVALID",
        message: "project name is required in non-interactive mode".into(),
        hint: "pass a project name, for example `labcoat init my-alkane`",
    }
}

pub fn prompt_project_name(
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<String, EnvelopeError> {
    loop {
        write!(output, "Project name: ").map_err(|error| io_error(Path::new("stderr"), error))?;
        output
            .flush()
            .map_err(|error| io_error(Path::new("stderr"), error))?;
        let mut line = String::new();
        let read = input
            .read_line(&mut line)
            .map_err(|error| io_error(Path::new("stdin"), error))?;
        if read == 0 {
            return Err(missing_project_name());
        }
        let name = line.trim();
        match validate_name(name, "project") {
            Ok(()) => return Ok(name.to_owned()),
            Err(error) => {
                writeln!(output, "{}", error.message)
                    .map_err(|error| io_error(Path::new("stderr"), error))?;
            }
        }
    }
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

pub(crate) fn find_project_root(start: &Path) -> Option<PathBuf> {
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
    validate_name(name, "contract")
}

fn validate_name(name: &str, kind: &str) -> Result<(), EnvelopeError> {
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
            message: format!("invalid {kind} name `{name}`"),
            hint: "use kebab-case beginning with a lowercase letter, for example `my-alkane`",
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
    use std::io::Cursor;

    fn test_parent(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("labcoat-{label}-{}", std::process::id()))
    }

    #[test]
    fn scaffolds_named_workspace_with_counter() {
        let parent = test_parent("init");
        std::fs::remove_dir_all(&parent).ok();
        std::fs::create_dir_all(&parent).unwrap();
        let result = init_in(&parent, "my-alkane").unwrap();
        let root = parent.join("my-alkane");
        assert_eq!(result["project"], "my-alkane");
        assert_eq!(result["template"], "default");
        assert_eq!(result["contract"], "counter");
        assert!(root.join("labcoat.toml").exists());
        assert!(std::fs::read_to_string(root.join("labcoat.toml"))
            .unwrap()
            .contains("network = \"labcoat\""));
        assert!(root.join("contracts/counter/Cargo.toml").exists());
        assert!(root.join("contracts/counter/src/lib.rs").exists());
        assert!(!root.join("crates").exists());
        let workspace_manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(workspace_manifest.contains("name = \"my-alkane\""));
        assert!(workspace_manifest.contains("members = [\"contracts/*\"]"));
        assert!(!workspace_manifest.contains("crates/*"));
        assert!(root.join("tests/counter.rs").exists());
        assert!(std::fs::read_to_string(root.join("tests/counter.rs"))
            .unwrap()
            .contains("for_contract(\"counter\")"));
        assert!(std::fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains(&format!("tag = \"cli-v{}\"", env!("CARGO_PKG_VERSION"))));
        assert!(std::fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains("git = \"https://github.com/jonatns/labcoat\""));
        std::fs::remove_dir_all(parent).ok();
    }

    /// Projects depend on `labcoat-test` from this git repository, and cargo
    /// parses every `Cargo.toml` in a git dependency's checkout. A template
    /// manifest with `name = "{{PROJECT_NAME}}"` makes cargo print an
    /// `invalid character` error in every downstream project, so templates
    /// must never be stored under the name `Cargo.toml`.
    #[test]
    fn templates_contain_no_cargo_manifests() {
        fn visit(dir: &Path, found: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, found);
                } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
                    found.push(path);
                }
            }
        }
        let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
        let mut found = Vec::new();
        visit(&templates, &mut found);
        assert!(found.is_empty(), "rename to Cargo.toml.tmpl: {found:?}");
    }

    #[test]
    fn rendered_manifests_have_no_placeholders() {
        let parent = test_parent("init-manifests");
        std::fs::remove_dir_all(&parent).ok();
        std::fs::create_dir_all(&parent).unwrap();
        init_in(&parent, "manifest-project").unwrap();
        let root = parent.join("manifest-project");
        new_contract_from(&root, "token").unwrap();
        for manifest in [
            "Cargo.toml",
            "contracts/counter/Cargo.toml",
            "contracts/token/Cargo.toml",
        ] {
            let contents = std::fs::read_to_string(root.join(manifest)).unwrap();
            assert!(!contents.contains("{{"), "{manifest} kept a placeholder");
        }
        assert!(
            std::fs::read_to_string(root.join("contracts/token/Cargo.toml"))
                .unwrap()
                .contains("name = \"token\"")
        );
        std::fs::remove_dir_all(parent).ok();
    }

    #[test]
    fn refuses_existing_destinations_and_invalid_project_names() {
        let parent = test_parent("existing");
        std::fs::remove_dir_all(&parent).ok();
        let root = parent.join("my-alkane");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("keep.txt"), "untouched").unwrap();
        assert!(init_in(&parent, "my-alkane").is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "untouched"
        );
        for invalid in [
            ".",
            "../alkane",
            "nested/alkane",
            "/alkane",
            "Bad_Name",
            "-alkane",
        ] {
            assert!(init_in(&parent, invalid).is_err(), "accepted {invalid}");
        }
        std::fs::remove_dir_all(parent).ok();
    }

    #[test]
    fn prompt_retries_invalid_names_and_handles_eof() {
        let mut input = Cursor::new(b"Bad_Name\nmy-alkane\n".to_vec());
        let mut output = Vec::new();
        assert_eq!(
            prompt_project_name(&mut input, &mut output).unwrap(),
            "my-alkane"
        );
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("Project name: ").count(), 2);
        assert!(output.contains("invalid project name `Bad_Name`"));

        let mut eof = Cursor::new(Vec::<u8>::new());
        assert_eq!(
            prompt_project_name(&mut eof, &mut Vec::new())
                .unwrap_err()
                .code,
            "CONFIG_INVALID"
        );
    }

    #[test]
    fn adds_contracts_without_overwriting_or_partial_collision_writes() {
        let parent = test_parent("contract-new");
        std::fs::remove_dir_all(&parent).ok();
        std::fs::create_dir_all(&parent).unwrap();
        init_in(&parent, "project").unwrap();
        let root = parent.join("project");
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
        std::fs::remove_dir_all(parent).ok();
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
        let manifest = include_str!("../templates/default/Cargo.toml.tmpl");
        assert!(manifest.contains(labcoat_core::compile::ALKANES_RS_REV));
        assert!(manifest.contains(
            &format!(
                "metashrew-support = {{ git = \"https://github.com/kungfuflex/metashrew\", tag = \"{}\" }}",
                labcoat_core::compile::METASHREW_TAG
            )
        ));
        assert!(!manifest.contains("sandshrewmetaprotocols/metashrew"));
        let workspace_manifest = include_str!("../../../Cargo.toml");
        assert!(workspace_manifest.contains("wasmi = \"=0.37.2\""));
    }

    #[test]
    fn project_toolchain_matches_the_workspace_toolchain() {
        let workspace_toolchain = include_str!("../../../rust-toolchain.toml");
        let channel = format!(
            "channel = \"{}\"",
            labcoat_core::compile::PROJECT_RUST_TOOLCHAIN
        );
        assert!(workspace_toolchain.contains(&channel));
        let template = include_str!("../templates/default/rust-toolchain.toml");
        assert!(template.contains("channel = \"{{RUST_TOOLCHAIN}}\""));
        assert!(template.contains("wasm32-unknown-unknown"));
        assert!(template.contains("wasm32-wasip1"));
    }

    #[test]
    fn init_writes_the_pinned_project_toolchain() {
        let parent =
            std::env::temp_dir().join(format!("labcoat-init-toolchain-{}", std::process::id()));
        std::fs::create_dir_all(&parent).unwrap();
        init_in(&parent, "toolchain-project").unwrap();
        let written =
            std::fs::read_to_string(parent.join("toolchain-project/rust-toolchain.toml")).unwrap();
        assert!(written.contains(&format!(
            "channel = \"{}\"",
            labcoat_core::compile::PROJECT_RUST_TOOLCHAIN
        )));
        assert!(!written.contains("{{"));
        std::fs::remove_dir_all(parent).ok();
    }
}
