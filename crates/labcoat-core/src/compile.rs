//! Cargo-native contract compilation and artifact production.

use crate::error::{LabcoatError, Result};
use crate::workspace::{ContractPackage, WorkspaceInfo};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Serialize;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// alkanes-rs pin — keep in sync with TOOLCHAIN.md and the project template.
pub const ALKANES_RS_REV: &str = "714843c416e2ab57352a33f05b8461cf3f540f5a";
/// metashrew rev matching alkanes-rs's Cargo.lock at the pinned commit.
pub const METASHREW_REV: &str = "22824e4ce8812751bd85b4dfff0da66b4ee025df";

/// Locate a C compiler with a WebAssembly backend. Apple Clang omits it,
/// while Homebrew LLVM and standard Linux Clang provide it.
pub fn wasm_c_compiler() -> Option<PathBuf> {
    for name in ["CC_wasm32_unknown_unknown", "CC"] {
        if let Some(value) = std::env::var_os(name).filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(value));
        }
    }
    let candidates = [
        PathBuf::from("/opt/homebrew/opt/llvm/bin/clang"),
        PathBuf::from("/usr/local/opt/llvm/bin/clang"),
        PathBuf::from("clang"),
    ];
    candidates.into_iter().find(|candidate| {
        std::process::Command::new(candidate)
            .arg("--print-targets")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).contains("wasm32"))
            .unwrap_or(false)
    })
}

fn wasi_include_dir() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("WASI_SYSROOT") {
        if let Some(include) = wasi_include_in(Path::new(&root)) {
            return Some(include);
        }
    }

    [
        PathBuf::from("/usr/include/wasm32-wasi"),
        PathBuf::from("/usr/local/share/wasi-sysroot/include"),
        PathBuf::from("/opt/wasi-sdk/share/wasi-sysroot/include"),
    ]
    .into_iter()
    .find(|path| path.is_dir())
}

fn wasi_include_in(root: &Path) -> Option<PathBuf> {
    [root.join("include/wasm32-wasi"), root.join("include")]
        .into_iter()
        .find(|path| path.is_dir())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileOutcome {
    pub name: String,
    pub wasm_path: String,
    pub wasm_gz_path: String,
    pub abi_path: String,
    pub wasm_sha256: String,
}

pub fn compile_packages(
    workspace: &WorkspaceInfo,
    packages: &[ContractPackage],
    out_dir: &Path,
    target: &str,
) -> Result<Vec<CompileOutcome>> {
    if packages.is_empty() {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            "no contract packages selected",
            "pass a discovered Cargo package name",
        ));
    }
    let mut packages = packages.to_vec();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    let _local_harness = explicit_local_labcoat_test_override(&workspace.root)?;

    let mut command = std::process::Command::new("cargo");
    command
        .args(["build", "--release", "--target", target])
        .current_dir(&workspace.root);
    if let Some(path) = local_labcoat_test_path() {
        let escaped = path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        command.arg("--config").arg(format!(
            "patch.'https://github.com/jonatns/labcoat'.labcoat-test.path=\"{escaped}\""
        ));
    }
    for package in &packages {
        command.arg("-p").arg(&package.name);
    }
    if target.starts_with("wasm32") {
        let compiler = wasm_c_compiler().ok_or_else(|| {
            LabcoatError::new(
                "COMPILE_FAILED",
                "no C compiler with a wasm32 backend was found",
                "install LLVM (`brew install llvm` on macOS, `apt install clang wasi-libc` on Linux)",
            )
        })?;
        command.env(format!("CC_{}", target.replace('-', "_")), compiler);
        if target == "wasm32-wasip1" {
            let cflags_key = format!("CFLAGS_{}", target.replace('-', "_"));
            if std::env::var_os(&cflags_key).is_none() {
                if let Some(include) = wasi_include_dir() {
                    command.env(cflags_key, format!("-isystem{}", include.display()));
                }
            }
        }
    }

    tracing::info!(
        target,
        packages = %packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(","),
        "building contract packages"
    );
    let output = command.output().map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("failed to run cargo build: {e}"),
            "install Cargo and run `labcoat doctor`",
        )
    })?;
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        return Err(LabcoatError::new(
            "COMPILE_FAILED",
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
            "fix the contract Cargo build errors above",
        ));
    }

    let out_dir = if out_dir.is_absolute() {
        out_dir.to_path_buf()
    } else {
        workspace.root.join(out_dir)
    };
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space"))?;

    let mut outcomes = Vec::with_capacity(packages.len());
    for package in &packages {
        let built_wasm = workspace
            .target_directory
            .join(target)
            .join("release")
            .join(format!("{}.wasm", package.lib_target_name));
        let wasm = std::fs::read(&built_wasm).map_err(|e| {
            LabcoatError::new(
                "COMPILE_FAILED",
                format!("built Wasm missing at {}: {e}", built_wasm.display()),
                "check the Cargo lib target name and build output",
            )
        })?;
        let abi = crate::abi::extract(&wasm)?;

        let wasm_path = out_dir.join(format!("{}.wasm", package.name));
        let wasm_gz_path = out_dir.join(format!("{}.wasm.gz", package.name));
        let abi_path = out_dir.join(format!("{}.abi.json", package.name));
        std::fs::write(&wasm_path, &wasm)
            .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space"))?;
        std::fs::write(&abi_path, &abi)
            .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space"))?;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&wasm)
            .and_then(|_| encoder.finish())
            .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "gzip failed"))
            .and_then(|gz| {
                std::fs::write(&wasm_gz_path, gz).map_err(|e| {
                    LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space")
                })
            })?;

        use sha2::Digest;
        outcomes.push(CompileOutcome {
            name: package.name.clone(),
            wasm_path: wasm_path.display().to_string(),
            wasm_gz_path: wasm_gz_path.display().to_string(),
            abi_path: abi_path.display().to_string(),
            wasm_sha256: hex::encode(sha2::Sha256::digest(&wasm)),
        });
    }
    Ok(outcomes)
}

/// Resolve the unpublished test harness while developing Labcoat from source.
fn local_labcoat_test_path() -> Option<PathBuf> {
    if std::env::var_os("LABCOAT_TEST_CRATE_PATH").is_some() {
        return None;
    }
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("labcoat-test");
    candidate.join("Cargo.toml").is_file().then_some(candidate)
}

/// Temporarily replace the release-tagged test harness with an explicit local
/// path. This is used by CI before the release tag exists. The project manifest
/// and lockfile are restored when the guard is dropped.
pub fn explicit_local_labcoat_test_override(
    workspace_root: &Path,
) -> Result<Option<LocalLabcoatTestOverride>> {
    let Some(path) = std::env::var_os("LABCOAT_TEST_CRATE_PATH") else {
        return Ok(None);
    };
    LocalLabcoatTestOverride::apply(workspace_root, &PathBuf::from(path))
}

pub struct LocalLabcoatTestOverride {
    manifest_path: PathBuf,
    manifest: Vec<u8>,
    lock_path: PathBuf,
    lock: Option<Vec<u8>>,
    backup_dir: PathBuf,
}

impl LocalLabcoatTestOverride {
    fn apply(workspace_root: &Path, harness_path: &Path) -> Result<Option<Self>> {
        Self::recover_stale(workspace_root)?;
        let manifest_path = workspace_root.join("Cargo.toml");
        let manifest = std::fs::read(&manifest_path).map_err(|error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot read {}: {error}", manifest_path.display()),
                "check project permissions",
            )
        })?;
        let source = String::from_utf8(manifest.clone()).map_err(|error| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!("{} is not UTF-8: {error}", manifest_path.display()),
                "repair the project Cargo.toml",
            )
        })?;
        let escaped = harness_path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let mut replaced = false;
        let rendered = source
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("labcoat-test = {")
                    && line.contains("git = \"https://github.com/jonatns/labcoat\"")
                {
                    replaced = true;
                    format!("labcoat-test = {{ path = \"{escaped}\" }}")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + if source.ends_with('\n') { "\n" } else { "" };
        if !replaced {
            return Ok(None);
        }

        let lock_path = workspace_root.join("Cargo.lock");
        let lock = std::fs::read(&lock_path).ok();
        let backup_dir = workspace_root.join(".labcoat/local-harness-override");
        std::fs::create_dir_all(&backup_dir).map_err(|error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot create {}: {error}", backup_dir.display()),
                "check project permissions",
            )
        })?;
        write_project_file(&backup_dir.join("Cargo.toml"), &manifest)?;
        write_project_file(
            &backup_dir.join("pid"),
            std::process::id().to_string().as_bytes(),
        )?;
        match &lock {
            Some(contents) => write_project_file(&backup_dir.join("Cargo.lock"), contents)?,
            None => write_project_file(&backup_dir.join("lock-absent"), b"")?,
        }
        let guard = Self {
            manifest_path,
            manifest,
            lock_path,
            lock,
            backup_dir,
        };
        write_project_file(&guard.manifest_path, rendered.as_bytes())?;
        Ok(Some(guard))
    }

    fn recover_stale(workspace_root: &Path) -> Result<()> {
        let backup_dir = workspace_root.join(".labcoat/local-harness-override");
        let manifest_backup = backup_dir.join("Cargo.toml");
        if !manifest_backup.exists() {
            return Ok(());
        }
        if std::fs::read_to_string(backup_dir.join("pid"))
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            == Some(std::process::id())
        {
            return Ok(());
        }
        let manifest = std::fs::read(&manifest_backup).map_err(|error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot recover {}: {error}", manifest_backup.display()),
                "restore the project Cargo.toml from version control",
            )
        })?;
        write_project_file(&workspace_root.join("Cargo.toml"), &manifest)?;
        let lock_backup = backup_dir.join("Cargo.lock");
        if lock_backup.exists() {
            let lock = std::fs::read(&lock_backup).map_err(|error| {
                LabcoatError::new(
                    "TOOLKIT_ERROR",
                    format!("cannot recover {}: {error}", lock_backup.display()),
                    "restore the project Cargo.lock from version control",
                )
            })?;
            write_project_file(&workspace_root.join("Cargo.lock"), &lock)?;
        } else if backup_dir.join("lock-absent").exists() {
            let _ = std::fs::remove_file(workspace_root.join("Cargo.lock"));
        }
        std::fs::remove_dir_all(&backup_dir).map_err(|error| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("cannot remove {}: {error}", backup_dir.display()),
                "check project permissions",
            )
        })
    }
}

impl Drop for LocalLabcoatTestOverride {
    fn drop(&mut self) {
        let _ = write_project_file(&self.manifest_path, &self.manifest);
        match &self.lock {
            Some(lock) => {
                let _ = write_project_file(&self.lock_path, lock);
            }
            None => {
                let _ = std::fs::remove_file(&self.lock_path);
            }
        }
        let _ = std::fs::remove_dir_all(&self.backup_dir);
    }
}

fn write_project_file(path: &Path, contents: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("labcoat-{}.tmp", std::process::id()));
    std::fs::write(&temporary, contents).map_err(|error| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("cannot write {}: {error}", temporary.display()),
            "check project permissions",
        )
    })?;
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("cannot replace {}: {error}", path.display()),
            "check project permissions",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_wasi_headers_below_a_sysroot() {
        let root = std::env::temp_dir().join(format!("labcoat-wasi-{}", std::process::id()));
        let include = root.join("include/wasm32-wasi");
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&include).unwrap();

        assert_eq!(wasi_include_in(&root), Some(include));

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn explicit_test_harness_override_restores_manifest_and_lockfile() {
        let root =
            std::env::temp_dir().join(format!("labcoat-harness-override-{}", std::process::id()));
        let harness = root.join("harness");
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&harness).unwrap();
        let manifest = b"[dev-dependencies]\nlabcoat-test = { git = \"https://github.com/jonatns/labcoat\", tag = \"cli-v9.9.9\" }\n";
        let lock = b"original lock\n";
        std::fs::write(root.join("Cargo.toml"), manifest).unwrap();
        std::fs::write(root.join("Cargo.lock"), lock).unwrap();

        {
            let _guard = LocalLabcoatTestOverride::apply(&root, &harness)
                .unwrap()
                .unwrap();
            let temporary = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
            assert!(temporary.contains("labcoat-test = { path = "));
            std::fs::write(root.join("Cargo.lock"), "temporary lock\n").unwrap();
        }

        assert_eq!(std::fs::read(root.join("Cargo.toml")).unwrap(), manifest);
        assert_eq!(std::fs::read(root.join("Cargo.lock")).unwrap(), lock);
        assert!(!root.join(".labcoat/local-harness-override").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn explicit_test_harness_override_recovers_an_interrupted_run() {
        let root =
            std::env::temp_dir().join(format!("labcoat-harness-recovery-{}", std::process::id()));
        let backup = root.join(".labcoat/local-harness-override");
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&backup).unwrap();
        let original = b"[dev-dependencies]\nlabcoat-test = { git = \"https://github.com/jonatns/labcoat\", tag = \"cli-v9.9.9\" }\n";
        std::fs::write(
            root.join("Cargo.toml"),
            "labcoat-test = { path = \"stale\" }\n",
        )
        .unwrap();
        std::fs::write(backup.join("Cargo.toml"), original).unwrap();
        std::fs::write(backup.join("lock-absent"), "").unwrap();
        std::fs::write(root.join("Cargo.lock"), "temporary lock\n").unwrap();

        LocalLabcoatTestOverride::recover_stale(&root).unwrap();

        assert_eq!(std::fs::read(root.join("Cargo.toml")).unwrap(), original);
        assert!(!root.join("Cargo.lock").exists());
        assert!(!backup.exists());
        std::fs::remove_dir_all(root).ok();
    }
}
