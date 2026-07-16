//! Contract compilation: .rs source → wasm32-unknown-unknown → .wasm.gz
//! artifact + regex-extracted ABI. A faithful port of the TS
//! AlkanesCompiler so both surfaces emit identical artifacts.

use crate::error::{LabcoatError, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Serialize;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// alkanes-rs pin — keep in sync with TOOLCHAIN.md (never a branch ref).
pub const ALKANES_RS_REV: &str = "5b7f43567b828d0bb7b8907ce78fa0242943c54d";
/// metashrew rev matching alkanes-rs's Cargo.lock at the pinned commit.
pub const METASHREW_REV: &str = "eca790ca1eeddc7cdac201b741637b8f18234924";

/// Locate a C compiler with a WebAssembly backend. Apple Clang omits it,
/// while Homebrew LLVM and standard Linux Clang provide it. secp256k1-sys
/// needs this compiler when contracts target WebAssembly.
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

#[derive(Debug, Clone, Serialize)]
pub struct AbiInput {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AbiMethod {
    pub opcode: u64,
    pub name: String,
    pub inputs: Vec<AbiInput>,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AbiStorageKey {
    pub key: String,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContractAbi {
    pub name: String,
    pub version: String,
    pub methods: Vec<AbiMethod>,
    pub storage: Vec<AbiStorageKey>,
    pub opcodes: std::collections::BTreeMap<String, u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileOutcome {
    pub name: String,
    /// Raw (uncompressed) wasm — what deploy's envelope needs.
    pub wasm_path: String,
    /// Gzipped artifact (kept for size/back-compat).
    pub wasm_gz_path: String,
    pub abi_path: String,
    pub wasm_sha256: String,
}

fn cargo_template() -> String {
    format!(
        r#"[package]
name = "alkanes-contract"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
alkanes-runtime = {{ git = "https://github.com/kungfuflex/alkanes-rs", rev = "{rev}" }}
alkanes-support = {{ git = "https://github.com/kungfuflex/alkanes-rs", rev = "{rev}" }}
metashrew-support = {{ git = "https://github.com/sandshrewmetaprotocols/metashrew", rev = "{meta}" }}
anyhow = "1.0"
"#,
        rev = ALKANES_RS_REV,
        meta = METASHREW_REV
    )
}

/// Compile one contract source file. Artifacts land in `<out_dir>/`
/// (`<name>.wasm`, `<name>.wasm.gz`, `<name>.abi.json`).
pub fn compile(source_path: &Path, name: Option<String>, out_dir: &Path) -> Result<CompileOutcome> {
    compile_for_target(source_path, name, out_dir, "wasm32-unknown-unknown")
}

/// Compile a contract for a specific WebAssembly target. The public CLI
/// uses `wasm32-unknown-unknown`; `labcoat test` uses `wasm32-wasip1` so
/// contracts can execute in the native host harness.
pub fn compile_for_target(
    source_path: &Path,
    name: Option<String>,
    out_dir: &Path,
    target: &str,
) -> Result<CompileOutcome> {
    let source = std::fs::read_to_string(source_path).map_err(|e| {
        LabcoatError::new(
            "CONFIG_INVALID",
            format!("cannot read {}: {}", source_path.display(), e),
            "pass a path to a .rs contract source",
        )
    })?;

    let abi = parse_abi(&source);
    let contract_name = name.unwrap_or_else(|| {
        source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&abi.name)
            .to_string()
    });

    // Scaffold a temp cargo project (mirrors the TS .labcoat/build_<id> dirs).
    let build_id = format!(
        "{:x}",
        std::process::id() as u64
            ^ std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
    );
    let temp_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".labcoat")
        .join(format!("build_{}", build_id));
    std::fs::create_dir_all(temp_dir.join("src"))
        .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space"))?;
    std::fs::write(temp_dir.join("Cargo.toml"), cargo_template())
        .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space"))?;
    std::fs::write(temp_dir.join("src/lib.rs"), &source)
        .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space"))?;

    let result = (|| {
        tracing::info!("Building contract in {}", temp_dir.display());
        let mut command = std::process::Command::new("cargo");
        command
            .args(["build", "--target", target, "--release"])
            .current_dir(&temp_dir);
        if target.starts_with("wasm32") {
            let compiler = wasm_c_compiler().ok_or_else(|| {
                LabcoatError::new(
                    "COMPILE_FAILED",
                    "no C compiler with a wasm32 backend was found",
                    "install LLVM (`brew install llvm` on macOS, `apt install clang` on Linux)",
                )
            })?;
            command.env(format!("CC_{}", target.replace('-', "_")), compiler);
        }
        let output = command.output().map_err(|e| {
            LabcoatError::new(
                "TOOLKIT_ERROR",
                format!("failed to run cargo: {}", e),
                "is the Rust toolchain with the wasm32-unknown-unknown target installed?",
            )
        })?;
        if !output.status.success() {
            return Err(LabcoatError::new(
                "COMPILE_FAILED",
                String::from_utf8_lossy(&output.stderr).to_string(),
                "fix the contract build errors above",
            ));
        }

        let wasm_built = temp_dir
            .join("target")
            .join(target)
            .join("release/alkanes_contract.wasm");
        let wasm = std::fs::read(&wasm_built).map_err(|e| {
            LabcoatError::new(
                "COMPILE_FAILED",
                format!("built wasm missing at {}: {}", wasm_built.display(), e),
                "check the cargo build output",
            )
        })?;

        std::fs::create_dir_all(out_dir)
            .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space"))?;

        let wasm_path = out_dir.join(format!("{}.wasm", contract_name));
        let wasm_gz_path = out_dir.join(format!("{}.wasm.gz", contract_name));
        let abi_path = out_dir.join(format!("{}.abi.json", contract_name));

        std::fs::write(&wasm_path, &wasm)
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

        let mut abi_named = abi.clone();
        if abi_named.name == "UnknownContract" {
            abi_named.name = contract_name.clone();
        }
        std::fs::write(&abi_path, serde_json::to_string_pretty(&abi_named).unwrap())
            .map_err(|e| LabcoatError::new("TOOLKIT_ERROR", e.to_string(), "check disk space"))?;

        use sha2::Digest;
        let hash = hex::encode(sha2::Sha256::digest(&wasm));

        Ok(CompileOutcome {
            name: contract_name.clone(),
            wasm_path: wasm_path.display().to_string(),
            wasm_gz_path: wasm_gz_path.display().to_string(),
            abi_path: abi_path.display().to_string(),
            wasm_sha256: hash,
        })
    })();

    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

/// Regex-equivalent port of the TS parseABI: #[opcode(n)] (+ optional
/// #[returns(T)]) enum variants with optional {field: Type} blocks; first
/// `pub struct` is the contract name; StoragePointer::from_keyword keys.
pub fn parse_abi(source: &str) -> ContractAbi {
    let mut methods = Vec::new();
    let mut opcodes = std::collections::BTreeMap::new();

    let bytes = source.as_bytes();
    let mut idx = 0;
    while let Some(found) = source[idx..].find("#[opcode(") {
        let start = idx + found + "#[opcode(".len();
        let Some(close) = source[start..].find(')') else {
            break;
        };
        let opcode_str = &source[start..start + close];
        let mut cursor = start + close;
        // skip to end of the attribute "]"
        if let Some(b) = source[cursor..].find(']') {
            cursor += b + 1;
        }
        idx = cursor;
        let Ok(opcode) = opcode_str.trim().parse::<u64>() else {
            continue;
        };

        // optional #[returns(T)]
        let mut outputs = Vec::new();
        let rest = skip_ws(bytes, cursor);
        let mut after_attrs = rest;
        if source[rest..].starts_with("#[returns(") {
            let rstart = rest + "#[returns(".len();
            if let Some(rclose) = source[rstart..].find(')') {
                outputs.push(source[rstart..rstart + rclose].trim().to_string());
                if let Some(b) = source[rstart + rclose..].find(']') {
                    after_attrs = rstart + rclose + b + 1;
                }
            }
        }

        // variant name
        let name_start = skip_ws(bytes, after_attrs);
        let mut name_end = name_start;
        while name_end < bytes.len()
            && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'_')
        {
            name_end += 1;
        }
        if name_end == name_start {
            continue;
        }
        let variant = source[name_start..name_end].to_string();

        // optional { fields }
        let mut inputs = Vec::new();
        let brace_start = skip_ws(bytes, name_end);
        if brace_start < bytes.len() && bytes[brace_start] == b'{' {
            if let Some(bclose) = source[brace_start..].find('}') {
                let block = &source[brace_start + 1..brace_start + bclose];
                for field in block.split(',') {
                    let mut parts = field.splitn(2, ':');
                    let (Some(fname), Some(ftype)) = (parts.next(), parts.next()) else {
                        continue;
                    };
                    let fname = fname.trim();
                    let ftype = ftype.trim();
                    if fname.is_empty() || ftype.is_empty() {
                        continue;
                    }
                    if !fname.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        continue;
                    }
                    inputs.push(AbiInput {
                        name: fname.to_string(),
                        type_: ftype.to_string(),
                    });
                }
            }
        }

        opcodes.insert(variant.clone(), opcode);
        methods.push(AbiMethod {
            opcode,
            name: variant,
            inputs,
            outputs,
        });
    }

    // Contract name = first `pub struct`
    let name = source
        .split("pub struct ")
        .nth(1)
        .and_then(|rest| {
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            let n = &rest[..end];
            if n.is_empty() {
                None
            } else {
                Some(n.to_string())
            }
        })
        .unwrap_or_else(|| "UnknownContract".to_string());

    // Storage keys
    let mut storage = Vec::new();
    let mut sidx = 0;
    while let Some(found) = source[sidx..].find("StoragePointer::from_keyword(\"") {
        let kstart = sidx + found + "StoragePointer::from_keyword(\"".len();
        let Some(kend) = source[kstart..].find('"') else {
            break;
        };
        storage.push(AbiStorageKey {
            key: source[kstart..kstart + kend].to_string(),
            type_: "Vec<u8>".to_string(),
        });
        sidx = kstart + kend;
    }

    ContractAbi {
        name,
        version: "1.0.0".to_string(),
        methods,
        storage,
        opcodes,
    }
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
pub struct MyToken(());

enum MyTokenMessage {
    #[opcode(0)]
    Initialize,

    #[opcode(1)]
    SetValue { value: u128 },

    #[opcode(99)]
    #[returns(String)]
    GetName,
}

fn ptr() {
    let p = StoragePointer::from_keyword("/value");
}
"#;

    #[test]
    fn parses_methods_and_metadata() {
        let abi = parse_abi(SOURCE);
        assert_eq!(abi.name, "MyToken");
        assert_eq!(abi.methods.len(), 3);
        assert_eq!(abi.methods[0].name, "Initialize");
        assert_eq!(abi.methods[1].inputs.len(), 1);
        assert_eq!(abi.methods[1].inputs[0].name, "value");
        assert_eq!(abi.methods[2].outputs, vec!["String".to_string()]);
        assert_eq!(abi.opcodes["GetName"], 99);
        assert_eq!(abi.storage.len(), 1);
        assert_eq!(abi.storage[0].key, "/value");
    }

    #[test]
    fn detected_wasm_compiler_reports_a_wasm_backend_when_available() {
        if let Some(compiler) = wasm_c_compiler() {
            let output = std::process::Command::new(compiler)
                .arg("--print-targets")
                .output()
                .unwrap();
            assert!(String::from_utf8_lossy(&output.stdout).contains("wasm32"));
        }
    }
}
