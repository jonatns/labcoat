//! ABI extraction from a contract's canonical `__meta` Wasm export.

use crate::error::{LabcoatError, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use wasmi::{
    Config, Engine, ExternType, Func, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
};

const ABI_FUEL: u64 = 100_000_000;
const MEMORY_LIMIT: usize = 43_554_432;
const MAX_ABI_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiComparison {
    pub matches: bool,
    pub local_sha256: String,
    pub deployed_sha256: String,
}

pub fn compare(local: &[u8], deployed: &[u8]) -> AbiComparison {
    use sha2::Digest;
    AbiComparison {
        matches: local == deployed,
        local_sha256: hex::encode(sha2::Sha256::digest(local)),
        deployed_sha256: hex::encode(sha2::Sha256::digest(deployed)),
    }
}

pub async fn fetch_deployed(
    config: &crate::system::ToolkitConfig,
    block: u128,
    tx: u128,
) -> Result<Vec<u8>> {
    use alkanes_cli_common::traits::AlkanesProvider;

    let provider = crate::system::connect(config, None, false).await?;
    let id = format!("{block}:{tx}");
    let bytes = AlkanesProvider::meta(&provider, &id, None)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;
    validate(&bytes).map_err(|message| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("deployed contract {id} returned invalid ABI metadata: {message}"),
            "verify the contract exports __meta and the indexer is synced",
        )
    })?;
    Ok(bytes)
}

#[derive(Debug, Deserialize)]
struct AbiDocument {
    contract: String,
    methods: Vec<AbiMethod>,
}

#[derive(Debug, Deserialize)]
struct AbiMethod {
    name: String,
    opcode: u128,
    params: Vec<AbiParam>,
    returns: String,
}

#[derive(Debug, Deserialize)]
struct AbiParam {
    name: String,
    #[serde(rename = "type")]
    type_: String,
}

struct HostState {
    limits: StoreLimits,
}

pub fn validate(bytes: &[u8]) -> std::result::Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("ABI is not UTF-8: {e}"))?;
    let abi: AbiDocument =
        serde_json::from_str(text).map_err(|e| format!("ABI is not valid JSON: {e}"))?;
    if abi.contract.trim().is_empty() {
        return Err("ABI `contract` must not be empty".into());
    }
    let mut opcodes = BTreeMap::new();
    for method in abi.methods {
        if method.name.trim().is_empty() {
            return Err("ABI method name must not be empty".into());
        }
        if method.returns.trim().is_empty() {
            return Err(format!(
                "ABI method `{}` has an empty return type",
                method.name
            ));
        }
        for param in &method.params {
            if param.name.trim().is_empty() || param.type_.trim().is_empty() {
                return Err(format!(
                    "ABI method `{}` has an invalid parameter",
                    method.name
                ));
            }
        }
        if let Some(previous) = opcodes.insert(method.opcode, method.name.clone()) {
            return Err(format!(
                "duplicate ABI opcode {} for `{}` and `{}`",
                method.opcode, previous, method.name
            ));
        }
    }
    Ok(())
}

pub fn extract_file(path: &Path) -> Result<Vec<u8>> {
    let wasm = std::fs::read(path).map_err(|e| {
        LabcoatError::new(
            "COMPILE_FAILED",
            format!("cannot read built Wasm {}: {e}", path.display()),
            "check the Cargo build output",
        )
    })?;
    extract(&wasm)
}

pub fn extract(wasm: &[u8]) -> Result<Vec<u8>> {
    extract_inner(wasm).map_err(|message| {
        LabcoatError::new(
            "COMPILE_FAILED",
            message,
            "export a pure `__meta: () -> i32` using declare_alkane!",
        )
    })
}

fn extract_inner(wasm: &[u8]) -> std::result::Result<Vec<u8>, String> {
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, wasm).map_err(|e| format!("invalid Wasm module: {e}"))?;
    let limits = StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build();
    let mut store = Store::new(&engine, HostState { limits });
    store.limiter(|state| &mut state.limits);
    store
        .set_fuel(ABI_FUEL)
        .map_err(|e| format!("cannot configure ABI fuel: {e}"))?;
    let mut linker = Linker::new(&engine);

    for import in module.imports() {
        let ExternType::Func(func_type) = import.ty() else {
            return Err(format!(
                "unsupported non-function Wasm import {}::{}",
                import.module(),
                import.name()
            ));
        };
        let import_name = format!("{}::{}", import.module(), import.name());
        let func = Func::new(
            &mut store,
            func_type.clone(),
            move |_caller, _params, _results| {
                Err(wasmi::Error::new(format!(
                    "ABI metadata called forbidden host import {import_name}"
                )))
            },
        );
        linker
            .define(import.module(), import.name(), func)
            .map_err(|e| format!("cannot define Wasm import: {e}"))?;
    }

    let instance = linker
        .instantiate(&mut store, &module)
        .and_then(|pre| pre.start(&mut store))
        .map_err(|e| format!("cannot instantiate contract Wasm: {e}"))?;
    let meta = instance
        .get_typed_func::<(), i32>(&store, "__meta")
        .map_err(|e| format!("contract is missing compatible __meta export: {e}"))?;
    let pointer = meta
        .call(&mut store, ())
        .map_err(|e| format!("__meta trapped: {e}"))?;
    let pointer = usize::try_from(pointer).map_err(|_| "__meta returned a negative pointer")?;
    if pointer < 4 {
        return Err("__meta returned an invalid array-buffer pointer".into());
    }
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| "contract does not export memory".to_string())?;
    let data = memory.data(&store);
    let length_bytes: [u8; 4] = data
        .get(pointer - 4..pointer)
        .ok_or_else(|| "__meta length pointer is outside Wasm memory".to_string())?
        .try_into()
        .map_err(|_| "invalid __meta length prefix".to_string())?;
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_ABI_BYTES {
        return Err(format!(
            "ABI is too large: {length} bytes (maximum {MAX_ABI_BYTES})"
        ));
    }
    let end = pointer
        .checked_add(length)
        .ok_or_else(|| "__meta pointer overflow".to_string())?;
    let bytes = data
        .get(pointer..end)
        .ok_or_else(|| "__meta payload is outside Wasm memory".to_string())?
        .to_vec();
    validate(&bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_upstream_shape_and_duplicate_opcodes() {
        validate(
            br#"{"contract":"Token","methods":[{"name":"mint","opcode":77,"params":[],"returns":"void"}]}"#,
        )
        .unwrap();
        let error = validate(
            br#"{"contract":"Token","methods":[{"name":"a","opcode":1,"params":[],"returns":"void"},{"name":"b","opcode":1,"params":[],"returns":"void"}]}"#,
        )
        .unwrap_err();
        assert!(error.contains("duplicate ABI opcode"));
    }

    #[test]
    fn rejects_wasm_without_meta() {
        let wasm = wat::parse_str("(module (memory (export \"memory\") 1))").unwrap();
        let error = extract(&wasm).unwrap_err();
        assert_eq!(error.code, "COMPILE_FAILED");
        assert!(error.message.contains("__meta"));
    }

    #[test]
    fn extracts_array_buffer_abi() {
        let abi = br#"{"contract":"Token","methods":[]}"#;
        let mut bytes = (abi.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(abi);
        let data = bytes
            .iter()
            .map(|byte| format!("\\{:02x}", byte))
            .collect::<String>();
        let wat = format!(
            "(module (memory (export \"memory\") 1) (data (i32.const 8) \"{data}\") (func (export \"__meta\") (result i32) i32.const 12))"
        );
        let wasm = wat::parse_str(wat).unwrap();
        assert_eq!(extract(&wasm).unwrap(), abi);
    }

    #[test]
    fn rejects_impure_meta_and_invalid_pointers() {
        let impure = wat::parse_str(
            r#"(module
                (import "env" "host" (func $host (result i32)))
                (memory (export "memory") 1)
                (func (export "__meta") (result i32) call $host))"#,
        )
        .unwrap();
        assert!(extract(&impure)
            .unwrap_err()
            .message
            .contains("host import"));

        let invalid = wat::parse_str(
            r#"(module (memory (export "memory") 1) (func (export "__meta") (result i32) i32.const 2))"#,
        )
        .unwrap();
        assert!(extract(&invalid)
            .unwrap_err()
            .message
            .contains("invalid array-buffer pointer"));
    }

    #[test]
    fn accepts_u128_opcodes_and_rejects_malformed_json() {
        validate(
            br#"{"contract":"Token","methods":[{"name":"max","opcode":340282366920938463463374607431768211455,"params":[],"returns":"void"}]}"#,
        )
        .unwrap();
        assert!(validate(b"not-json").unwrap_err().contains("valid JSON"));
    }

    #[test]
    fn rejects_oversized_abi_before_reading_payload() {
        let length = ((MAX_ABI_BYTES + 1) as u32).to_le_bytes();
        let data = length
            .iter()
            .map(|byte| format!("\\{:02x}", byte))
            .collect::<String>();
        let wat = format!(
            "(module (memory (export \"memory\") 1) (data (i32.const 0) \"{data}\") (func (export \"__meta\") (result i32) i32.const 4))"
        );
        let error = extract(&wat::parse_str(wat).unwrap()).unwrap_err();
        assert!(error.message.contains("ABI is too large"));
    }

    #[test]
    fn compares_exact_deployed_abi_bytes() {
        assert!(compare(b"same", b"same").matches);
        let mismatch = compare(b"local", b"deployed");
        assert!(!mismatch.matches);
        assert_ne!(mismatch.local_sha256, mismatch.deployed_sha256);
    }
}
