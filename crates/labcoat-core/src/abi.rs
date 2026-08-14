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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AbiDocument {
    pub contract: String,
    pub methods: Vec<AbiMethod>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AbiMethod {
    pub name: String,
    pub opcode: u128,
    pub params: Vec<AbiParam>,
    pub returns: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AbiParam {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMethod {
    pub name: String,
    pub opcode: u128,
    pub cellpack_args: Vec<u128>,
}

/// Call or constructor arguments: positional values in ABI parameter order,
/// or named values matched to ABI parameter names before encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallArgs {
    Positional(Vec<String>),
    Named(Vec<(String, String)>),
}

impl CallArgs {
    pub fn is_empty(&self) -> bool {
        match self {
            CallArgs::Positional(values) => values.is_empty(),
            CallArgs::Named(values) => values.is_empty(),
        }
    }
}

struct HostState {
    limits: StoreLimits,
}

pub fn parse(bytes: &[u8]) -> std::result::Result<AbiDocument, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("ABI is not UTF-8: {e}"))?;
    let abi: AbiDocument =
        serde_json::from_str(text).map_err(|e| format!("ABI is not valid JSON: {e}"))?;
    if abi.contract.trim().is_empty() {
        return Err("ABI `contract` must not be empty".into());
    }
    let mut names = BTreeMap::new();
    let mut opcodes = BTreeMap::new();
    for method in &abi.methods {
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
        if let Some(previous_opcode) = names.insert(method.name.clone(), method.opcode) {
            return Err(format!(
                "duplicate ABI method name `{}` for opcodes {} and {}",
                method.name, previous_opcode, method.opcode
            ));
        }
    }
    Ok(abi)
}

pub fn validate(bytes: &[u8]) -> std::result::Result<(), String> {
    parse(bytes).map(|_| ())
}

/// Resolve an exact ABI method name and encode one argument per ABI
/// parameter into the contract's raw cellpack representation.
pub fn resolve_method(bytes: &[u8], selector: &str, args: &CallArgs) -> Result<ResolvedMethod> {
    let abi = parse(bytes).map_err(|message| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("cannot resolve method from invalid ABI metadata: {message}"),
            "verify the contract exports valid __meta ABI metadata",
        )
    })?;
    let method = abi
        .methods
        .iter()
        .find(|method| method.name == selector)
        .ok_or_else(|| {
            let available = abi
                .methods
                .iter()
                .map(|method| method.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let message = if available.is_empty() {
                format!("ABI method `{selector}` was not found; the contract exposes no methods")
            } else {
                format!("ABI method `{selector}` was not found; available methods: {available}")
            };
            let hint = if selector.contains('(') || selector.contains(')') {
                "pass the bare ABI method name without parentheses, for example `increment`"
            } else {
                "pass an available ABI method name or a numeric opcode"
            };
            LabcoatError::new("CONFIG_INVALID", message, hint)
        })?;

    let ordered = ordered_args(method, args)?;
    let cellpack_args = encode_method_args(method, &ordered)?;

    Ok(ResolvedMethod {
        name: method.name.clone(),
        opcode: method.opcode,
        cellpack_args,
    })
}

/// Resolve the constructor (initializer opcode 0) and encode one argument
/// per ABI parameter. Returns Ok(None) when the ABI declares no opcode-0
/// method — callers fall back to raw u128 args (positional only).
pub fn resolve_constructor(bytes: &[u8], args: &CallArgs) -> Result<Option<ResolvedMethod>> {
    let abi = parse(bytes).map_err(|message| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("cannot resolve constructor from invalid ABI metadata: {message}"),
            "verify the contract exports valid __meta ABI metadata",
        )
    })?;
    let Some(method) = abi.methods.iter().find(|method| method.opcode == 0) else {
        return Ok(None);
    };
    let ordered = ordered_args(method, args)?;
    let cellpack_args = encode_method_args(method, &ordered)?;
    Ok(Some(ResolvedMethod {
        name: method.name.clone(),
        opcode: 0,
        cellpack_args,
    }))
}

/// Positional args in the ABI's parameter order: pass-through for
/// positional input, name-matched reordering for named input.
fn ordered_args(method: &AbiMethod, args: &CallArgs) -> Result<Vec<String>> {
    let named = match args {
        CallArgs::Positional(values) => return Ok(values.clone()),
        CallArgs::Named(values) => values,
    };
    let expected = method
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param.type_))
        .collect::<Vec<_>>()
        .join(", ");
    for (name, _) in named {
        if !method.params.iter().any(|param| &param.name == name) {
            return Err(LabcoatError::new(
                "CONFIG_INVALID",
                format!(
                    "method `{}` has no parameter `{name}` — expected parameters: {expected}",
                    method.name
                ),
                "name each arg after an ABI parameter, or use positional args = [...]",
            ));
        }
    }
    let mut ordered = Vec::with_capacity(method.params.len());
    let mut missing = Vec::new();
    for param in &method.params {
        match named.iter().find(|(name, _)| name == &param.name) {
            Some((_, value)) => ordered.push(value.clone()),
            None => missing.push(param.name.as_str()),
        }
    }
    if !missing.is_empty() {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!(
                "method `{}` is missing named arg(s): {} — expected parameters: {expected}",
                method.name,
                missing.join(", ")
            ),
            "name each arg after an ABI parameter, or use positional args = [...]",
        ));
    }
    Ok(ordered)
}

/// Encode one shell argument per ABI parameter into the method's raw
/// cellpack representation.
fn encode_method_args(method: &AbiMethod, args: &[String]) -> Result<Vec<u128>> {
    if args.len() != method.params.len() {
        let expected = method
            .params
            .iter()
            .map(|param| format!("{}: {}", param.name, param.type_))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!(
                "method `{}` expects {} parameter(s) ({expected}), received {}",
                method.name,
                method.params.len(),
                args.len()
            ),
            "pass one shell argument per ABI parameter, or use a numeric opcode for raw cellpack arguments",
        ));
    }

    let mut cellpack_args = Vec::new();
    for (param, value) in method.params.iter().zip(args) {
        let encoded = encode_parameter(&param.type_, value).map_err(|reason| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!(
                    "invalid parameter `{}` for method `{}`: expected {}, received `{}` ({reason})",
                    param.name, method.name, param.type_, value
                ),
                "pass a value matching the ABI type, or use a numeric opcode for raw cellpack arguments",
            )
        })?;
        cellpack_args.extend(encoded);
    }
    Ok(cellpack_args)
}

/// How constructor args were encoded for a deploy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConstructorEncoding {
    /// One shell argument per ABI constructor parameter.
    AbiTyped,
    /// Raw u128 / 0x-hex cellpack values (no ABI constructor available).
    Raw,
}

#[derive(Debug, Clone)]
pub struct EncodedConstructor {
    pub cellpack: Vec<u128>,
    pub encoding: ConstructorEncoding,
    /// The ABI constructor's method name, when one was used.
    pub method: Option<String>,
    /// A caller-facing warning: the raw fallback was taken with args present.
    pub note: Option<String>,
}

/// Encode deploy constructor args against the artifact's ABI (the sibling
/// `.abi.json` written by `labcoat build`, else `__meta` extracted from the
/// wasm itself). Positional args fall back to raw cellpack values when no
/// ABI constructor is available; named args require a typed constructor;
/// ABI type/arity mismatches are hard errors.
pub fn encode_constructor(wasm_path: &Path, args: &CallArgs) -> Result<EncodedConstructor> {
    let abi_bytes = artifact_abi_bytes(wasm_path);
    let mut note = None;
    if let Some(bytes) = abi_bytes {
        if let Some(resolved) = resolve_constructor(&bytes, args)? {
            return Ok(EncodedConstructor {
                cellpack: resolved.cellpack_args,
                encoding: ConstructorEncoding::AbiTyped,
                method: Some(resolved.name),
                note: None,
            });
        }
        if matches!(args, CallArgs::Named(_)) {
            return Err(LabcoatError::new(
                "CONFIG_INVALID",
                format!(
                    "named args require an opcode-0 constructor in the ABI, but {} declares none",
                    wasm_path.display()
                ),
                "use positional args = [...] or add an opcode-0 initializer to the contract",
            ));
        }
        if !args.is_empty() {
            note = Some(
                "ABI declares no opcode-0 constructor — encoding args as raw cellpack values"
                    .to_string(),
            );
        }
    } else {
        if matches!(args, CallArgs::Named(_)) {
            return Err(LabcoatError::new(
                "CONFIG_INVALID",
                format!(
                    "named args require typed ABI constructor metadata, but no ABI is available for {}",
                    wasm_path.display()
                ),
                "use positional args = [...] or export __meta from the contract",
            ));
        }
        if !args.is_empty() {
            note =
                Some("no ABI metadata available — encoding args as raw cellpack values".to_string());
        }
    }
    let CallArgs::Positional(values) = args else {
        unreachable!("named args error out above the raw fallback");
    };
    Ok(EncodedConstructor {
        cellpack: values
            .iter()
            .map(|a| parse_raw_arg(a))
            .collect::<Result<Vec<_>>>()?,
        encoding: ConstructorEncoding::Raw,
        method: None,
        note,
    })
}

/// A raw cellpack value on a manifest-driven path. Unlike `crate::parse_arg`
/// (whose string→LE-u128 encoding is a deliberate shell convenience), this
/// rejects `block:tx`-shaped values: silently encoding an AlkaneId's ASCII
/// bytes as one u128 would corrupt the cellpack with no diagnostic.
pub(crate) fn parse_raw_arg(value: &str) -> Result<u128> {
    if value.contains(':') {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!(
                "arg `{value}` looks like an AlkaneId but no typed ABI parameter is available — raw cellpack encoding would corrupt it"
            ),
            "add ABI metadata (export __meta), or pass block and tx as separate integer args",
        ));
    }
    crate::parse_arg(value)
}

/// The artifact's ABI JSON: the sibling `.abi.json` if present, else
/// extracted from the wasm's `__meta` export. None when neither works.
fn artifact_abi_bytes(wasm_path: &Path) -> Option<Vec<u8>> {
    let abi_path = wasm_path.with_extension("abi.json");
    if let Ok(bytes) = std::fs::read(&abi_path) {
        return Some(bytes);
    }
    extract_file(wasm_path).ok()
}

fn encode_parameter(type_: &str, value: &str) -> std::result::Result<Vec<u128>, String> {
    match type_ {
        "u128" => parse_typed_u128(value).map(|value| vec![value]),
        "String" => encode_string(value),
        "AlkaneId" => encode_alkane_id(value),
        unsupported => Err(format!(
            "ABI type `{unsupported}` is not supported by named invocation"
        )),
    }
}

fn parse_typed_u128(value: &str) -> std::result::Result<u128, String> {
    if let Some(hex) = value.strip_prefix("0x") {
        if hex.is_empty() {
            return Err("hexadecimal values require digits after `0x`".into());
        }
        return u128::from_str_radix(hex, 16)
            .map_err(|_| "value must be a hexadecimal u128".into());
    }
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("value must be a decimal or 0x-prefixed hexadecimal u128".into());
    }
    value
        .parse()
        .map_err(|_| "decimal value must fit in u128".into())
}

fn encode_string(value: &str) -> std::result::Result<Vec<u128>, String> {
    if value.as_bytes().contains(&0) {
        return Err("strings cannot contain a null byte".into());
    }
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    Ok(bytes
        .chunks(16)
        .map(|chunk| {
            let mut padded = [0_u8; 16];
            padded[..chunk.len()].copy_from_slice(chunk);
            u128::from_le_bytes(padded)
        })
        .collect())
}

fn encode_alkane_id(value: &str) -> std::result::Result<Vec<u128>, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err("AlkaneId must use decimal `block:tx` syntax".into());
    }
    let block = parts[0]
        .parse::<u128>()
        .map_err(|_| "AlkaneId block must be a decimal u128".to_string())?;
    let tx = parts[1]
        .parse::<u128>()
        .map_err(|_| "AlkaneId tx must be a decimal u128".to_string())?;
    Ok(vec![block, tx])
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

    fn pos(args: &[&str]) -> CallArgs {
        CallArgs::Positional(args.iter().map(|a| a.to_string()).collect())
    }

    fn named(args: &[(&str, &str)]) -> CallArgs {
        CallArgs::Named(
            args.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

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

        let error = validate(
            br#"{"contract":"Token","methods":[{"name":"mint","opcode":1,"params":[],"returns":"void"},{"name":"mint","opcode":2,"params":[],"returns":"void"}]}"#,
        )
        .unwrap_err();
        assert!(error.contains("duplicate ABI method name `mint`"));
    }

    #[test]
    fn resolves_and_encodes_the_opcode_zero_constructor() {
        let abi = br#"{"contract":"Token","methods":[{"name":"initialize","opcode":0,"params":[{"type":"u128","name":"variant"},{"type":"u128","name":"supply"}],"returns":"void"},{"name":"mint","opcode":77,"params":[],"returns":"void"}]}"#;

        let resolved = resolve_constructor(abi, &pos(&["1", "100"]))
            .unwrap()
            .expect("opcode-0 constructor exists");
        assert_eq!(resolved.name, "initialize");
        assert_eq!(resolved.opcode, 0);
        assert_eq!(resolved.cellpack_args, vec![1, 100]);

        // Arg-count mismatches are hard errors, not silent fallbacks.
        let error = resolve_constructor(abi, &pos(&["1"])).unwrap_err();
        assert_eq!(error.code, "CONFIG_INVALID");
        assert!(error.message.contains("expects 2 parameter(s)"));
    }

    #[test]
    fn missing_constructor_resolves_to_none() {
        let abi = br#"{"contract":"Token","methods":[{"name":"mint","opcode":77,"params":[],"returns":"void"}]}"#;
        assert!(resolve_constructor(abi, &pos(&[])).unwrap().is_none());
    }

    #[test]
    fn resolves_counter_method_names_to_opcodes() {
        let abi = br#"{"contract":"Counter","methods":[{"name":"initialize","opcode":0,"params":[],"returns":"void"},{"name":"increment","opcode":1,"params":[],"returns":"u128"},{"name":"get_count","opcode":2,"params":[],"returns":"u128"}]}"#;

        assert_eq!(resolve_method(abi, "initialize", &pos(&[])).unwrap().opcode, 0);
        assert_eq!(resolve_method(abi, "increment", &pos(&[])).unwrap().opcode, 1);
        assert_eq!(resolve_method(abi, "get_count", &pos(&[])).unwrap().opcode, 2);
    }

    #[test]
    fn encodes_typed_u128_and_alkane_id_parameters() {
        let abi = br#"{"contract":"Token","methods":[{"name":"configure","opcode":9,"params":[{"name":"amount","type":"u128"},{"name":"limit","type":"u128"},{"name":"owner","type":"AlkaneId"}],"returns":"void"}]}"#;
        let resolved = resolve_method(abi, "configure", &pos(&["42", "0xff", "2:3"])).unwrap();
        assert_eq!(resolved.name, "configure");
        assert_eq!(resolved.opcode, 9);
        assert_eq!(resolved.cellpack_args, vec![42, 255, 2, 3]);
    }

    #[test]
    fn encodes_empty_multicell_and_exact_word_strings() {
        let abi = br#"{"contract":"Registry","methods":[{"name":"set_name","opcode":3,"params":[{"name":"name","type":"String"}],"returns":"void"}]}"#;

        let empty = resolve_method(abi, "set_name", &pos(&[""])).unwrap();
        assert_eq!(empty.cellpack_args, vec![0]);

        let multicell = resolve_method(
            abi,
            "set_name",
            &pos(&["a string longer than sixteen bytes"]),
        )
        .unwrap();
        assert!(multicell.cellpack_args.len() > 1);
        assert_eq!(multicell.cellpack_args.last().unwrap().to_le_bytes()[15], 0);

        let exact = resolve_method(abi, "set_name", &pos(&["1234567890abcdef"])).unwrap();
        assert_eq!(exact.cellpack_args.len(), 2);
        assert_eq!(exact.cellpack_args[1], 0);
    }

    #[test]
    fn reports_named_method_and_parameter_errors() {
        let abi = br#"{"contract":"Token","methods":[{"name":"mint","opcode":7,"params":[{"name":"amount","type":"u128"}],"returns":"void"},{"name":"batch","opcode":8,"params":[{"name":"amounts","type":"Vec<u128>"}],"returns":"void"}]}"#;

        let unknown = resolve_method(abi, "burn", &pos(&[])).unwrap_err();
        assert_eq!(unknown.code, "CONFIG_INVALID");
        assert!(unknown.message.contains("available methods: mint, batch"));

        let expression = resolve_method(abi, "mint(1)", &pos(&[])).unwrap_err();
        assert!(expression.hint.contains("without parentheses"));

        let arity = resolve_method(abi, "mint", &pos(&[])).unwrap_err();
        assert!(arity.message.contains("amount: u128"));
        assert!(arity.message.contains("received 0"));

        let bad_u128 = resolve_method(abi, "mint", &pos(&["many"])).unwrap_err();
        assert!(bad_u128.message.contains("parameter `amount`"));
        assert!(bad_u128.message.contains("expected u128"));

        let bad_hex = resolve_method(abi, "mint", &pos(&["0xgg"])).unwrap_err();
        assert!(bad_hex.message.contains("hexadecimal u128"));

        let unsupported = resolve_method(abi, "batch", &pos(&["[1,2]"])).unwrap_err();
        assert!(unsupported.message.contains("Vec<u128>"));
        assert!(unsupported.hint.contains("numeric opcode"));
    }

    #[test]
    fn rejects_invalid_typed_values() {
        let abi = br#"{"contract":"Registry","methods":[{"name":"set","opcode":4,"params":[{"name":"owner","type":"AlkaneId"},{"name":"name","type":"String"}],"returns":"void"}]}"#;

        let bad_id = resolve_method(abi, "set", &pos(&["2:3:4", "name"])).unwrap_err();
        assert!(bad_id.message.contains("AlkaneId"));

        let null = resolve_method(abi, "set", &pos(&["2:3", "a\0b"])).unwrap_err();
        assert!(null.message.contains("null byte"));
    }

    #[test]
    fn reorders_named_args_against_abi_parameter_order() {
        let abi = br#"{"contract":"Series","methods":[{"name":"initialize","opcode":0,"params":[{"name":"underlying","type":"AlkaneId"},{"name":"strike","type":"u128"},{"name":"supply","type":"u128"}],"returns":"void"}]}"#;
        let resolved = resolve_constructor(
            abi,
            &named(&[("supply", "100"), ("underlying", "4:65011"), ("strike", "75")]),
        )
        .unwrap()
        .expect("opcode-0 constructor exists");
        assert_eq!(resolved.cellpack_args, vec![4, 65011, 75, 100]);
    }

    #[test]
    fn reports_missing_and_unknown_named_args() {
        let abi = br#"{"contract":"Series","methods":[{"name":"initialize","opcode":0,"params":[{"name":"underlying","type":"AlkaneId"},{"name":"strike","type":"u128"}],"returns":"void"}]}"#;

        let missing = resolve_constructor(abi, &named(&[("underlying", "4:65011")])).unwrap_err();
        assert_eq!(missing.code, "CONFIG_INVALID");
        assert!(missing.message.contains("missing named arg(s): strike"));
        assert!(missing.message.contains("underlying: AlkaneId, strike: u128"));

        let unknown = resolve_constructor(abi, &named(&[("strke", "75")])).unwrap_err();
        assert!(unknown.message.contains("has no parameter `strke`"));
        assert!(unknown.message.contains("underlying: AlkaneId, strike: u128"));
    }

    #[test]
    fn named_constructor_args_require_a_typed_abi() {
        let missing_abi = encode_constructor(
            Path::new("/nonexistent/contract.wasm"),
            &named(&[("supply", "100")]),
        )
        .unwrap_err();
        assert_eq!(missing_abi.code, "CONFIG_INVALID");
        assert!(missing_abi.message.contains("no ABI is available"));

        let dir = std::env::temp_dir().join(format!("labcoat-abi-named-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("contract.abi.json"),
            br#"{"contract":"Token","methods":[{"name":"mint","opcode":77,"params":[],"returns":"void"}]}"#,
        )
        .unwrap();
        let no_constructor =
            encode_constructor(&dir.join("contract.wasm"), &named(&[("supply", "100")]))
                .unwrap_err();
        assert!(no_constructor
            .message
            .contains("opcode-0 constructor in the ABI"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn raw_fallback_rejects_alkane_id_shaped_args() {
        let corrupt = encode_constructor(
            Path::new("/nonexistent/contract.wasm"),
            &pos(&["4:65011"]),
        )
        .unwrap_err();
        assert_eq!(corrupt.code, "CONFIG_INVALID");
        assert!(corrupt.message.contains("raw cellpack encoding would corrupt it"));

        let plain = encode_constructor(Path::new("/nonexistent/contract.wasm"), &pos(&["7"]))
            .unwrap();
        assert_eq!(plain.cellpack, vec![7]);
        assert_eq!(plain.encoding, ConstructorEncoding::Raw);
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
