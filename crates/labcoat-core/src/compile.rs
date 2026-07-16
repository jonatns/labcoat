//! Cargo-native contract compilation and artifact production.

use crate::error::{LabcoatError, Result};
use crate::workspace::{ContractPackage, WorkspaceInfo};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Serialize;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// alkanes-rs pin — keep in sync with TOOLCHAIN.md and the project template.
pub const ALKANES_RS_REV: &str = "5b7f43567b828d0bb7b8907ce78fa0242943c54d";
/// metashrew rev matching alkanes-rs's Cargo.lock at the pinned commit.
pub const METASHREW_REV: &str = "eca790ca1eeddc7cdac201b741637b8f18234924";

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

    let mut command = std::process::Command::new("cargo");
    command
        .args(["build", "--release", "--target", target])
        .current_dir(&workspace.root);
    if let Some(path) = local_labcoat_test_path() {
        let escaped = path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        command
            .arg("--config")
            .arg(format!("patch.crates-io.labcoat-test.path=\"{escaped}\""));
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
    if let Some(path) = std::env::var_os("LABCOAT_TEST_CRATE_PATH") {
        return Some(PathBuf::from(path));
    }
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("labcoat-test");
    candidate.join("Cargo.toml").is_file().then_some(candidate)
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
}
