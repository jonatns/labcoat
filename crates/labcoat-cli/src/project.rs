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
        "contracts/Example.rs",
        include_str!("../templates/default/contracts/Example.rs"),
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
        std::fs::write(&destination, contents).map_err(|e| io_error(&destination, e))?;
    }

    Ok(serde_json::json!({
        "directory": target,
        "template": "default",
        "files": FILES.iter().map(|(path, _)| *path).collect::<Vec<_>>(),
        "next": ["labcoat test", "labcoat up", "labcoat wallet init"]
    }))
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
        assert!(root.join("labcoat.toml").exists());
        assert!(root.join("contracts/Example.rs").exists());
        assert!(root.join("tests/example.rs").exists());
        assert!(init(root.to_str(), false).is_err());
        assert!(init(root.to_str(), true).is_ok());
        std::fs::remove_dir_all(root).ok();
    }
}
