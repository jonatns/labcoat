//! Native host-side integration testing for Labcoat contracts.
//!
//! `labcoat test` compiles WASIp1 WebAssembly modules and points this harness
//! at the resulting artifact directory through `LABCOAT_TEST_ARTIFACT_DIR`.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wasmtime::{Caller, Engine, Extern, Linker, Module, Store};
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    U128(u128),
    String(String),
}

impl From<u128> for Value {
    fn from(value: u128) -> Self {
        Self::U128(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlkaneTransfer {
    pub block: u128,
    pub tx: u128,
    pub value: u128,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChainContext {
    pub myself: (u128, u128),
    pub caller: (u128, u128),
    pub vout: u128,
    pub incoming: Vec<AlkaneTransfer>,
}

impl ChainContext {
    pub fn deterministic() -> Self {
        Self {
            myself: (1, 1),
            caller: (2, 2),
            vout: 0,
            incoming: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallResult {
    pub transfers: Vec<AlkaneTransfer>,
    pub data: Vec<u8>,
}

impl CallResult {
    pub fn data_text(&self) -> String {
        String::from_utf8_lossy(&self.data)
            .trim_end_matches('\0')
            .to_owned()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    #[error("contract artifact not found: {0}")]
    ArtifactMissing(PathBuf),
    #[error("method `{0}` is not present in the contract ABI")]
    UnknownMethod(String),
    #[error("contract trapped or reverted: {0}")]
    Revert(String),
    #[error("invalid contract response: {0}")]
    InvalidResponse(String),
    #[error(transparent)]
    Runtime(#[from] anyhow::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct ContractAbi {
    methods: Vec<AbiMethod>,
}

#[derive(Debug, Deserialize)]
struct AbiMethod {
    name: String,
    opcode: u128,
}

struct HostState {
    wasi: WasiP1Ctx,
    context: Vec<u8>,
    storage: BTreeMap<Vec<u8>, Vec<u8>>,
}

pub struct ContractHarness {
    engine: Engine,
    module: Module,
    abi: ContractAbi,
    context: ChainContext,
    storage: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl ContractHarness {
    pub fn for_contract(name: &str) -> Result<Self, HarnessError> {
        let root = std::env::var_os("LABCOAT_TEST_ARTIFACT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".labcoat/test-artifacts"));
        Self::from_files(
            root.join(format!("{}.wasm", name)),
            root.join(format!("{}.abi.json", name)),
        )
    }

    pub fn from_files(
        wasm_path: impl AsRef<Path>,
        abi_path: impl AsRef<Path>,
    ) -> Result<Self, HarnessError> {
        let wasm_path = wasm_path.as_ref();
        if !wasm_path.exists() {
            return Err(HarnessError::ArtifactMissing(wasm_path.to_path_buf()));
        }
        let abi_path = abi_path.as_ref();
        if !abi_path.exists() {
            return Err(HarnessError::ArtifactMissing(abi_path.to_path_buf()));
        }
        let engine = Engine::default();
        let module = Module::from_file(&engine, wasm_path).map_err(HarnessError::Runtime)?;
        let abi = serde_json::from_slice(&std::fs::read(abi_path)?)?;
        Ok(Self {
            engine,
            module,
            abi,
            context: ChainContext::deterministic(),
            storage: BTreeMap::new(),
        })
    }

    pub fn context(&self) -> &ChainContext {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut ChainContext {
        &mut self.context
    }

    pub fn set_context(&mut self, context: ChainContext) -> &mut Self {
        self.context = context;
        self
    }

    /// Return the value currently stored for a raw contract storage key.
    pub fn storage_value(&self, key: impl AsRef<[u8]>) -> Option<&[u8]> {
        self.storage.get(key.as_ref()).map(Vec::as_slice)
    }

    /// Seed or replace a raw contract storage value before executing a call.
    pub fn set_storage(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> &mut Self {
        self.storage
            .insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        self
    }

    pub fn call_method(
        &mut self,
        method: &str,
        args: &[Value],
    ) -> Result<CallResult, HarnessError> {
        let opcode = self
            .abi
            .methods
            .iter()
            .find(|entry| entry.name == method)
            .map(|entry| entry.opcode)
            .ok_or_else(|| HarnessError::UnknownMethod(method.to_owned()))?;
        self.call_opcode(opcode, args)
    }

    pub fn call_opcode(
        &mut self,
        opcode: u128,
        args: &[Value],
    ) -> Result<CallResult, HarnessError> {
        let context = serialize_context(&self.context, opcode, args)?;
        let mut linker: Linker<HostState> = Linker::new(&self.engine);
        preview1::add_to_linker_sync(&mut linker, |state| &mut state.wasi)
            .map_err(HarnessError::Runtime)?;
        add_alkanes_imports(&mut linker)?;

        let state = HostState {
            wasi: WasiCtxBuilder::new().build_p1(),
            context,
            storage: self.storage.clone(),
        };
        let mut store = Store::new(&self.engine, state);
        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(HarnessError::Runtime)?;
        if let Ok(initialize) = instance.get_typed_func::<(), ()>(&mut store, "_initialize") {
            initialize
                .call(&mut store, ())
                .map_err(|e| HarnessError::Revert(format!("{e:#}")))?;
        }
        let execute = instance
            .get_typed_func::<(), i32>(&mut store, "__execute")
            .map_err(HarnessError::Runtime)?;
        let pointer = execute
            .call(&mut store, ())
            .map_err(|e| HarnessError::Revert(format!("{e:#}")))?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| HarnessError::InvalidResponse("missing memory export".into()))?;
        let decoded = decode_response(memory.data(&store), pointer)?;
        self.storage.extend(decoded.storage);
        Ok(decoded.result)
    }
}

fn add_alkanes_imports(linker: &mut Linker<HostState>) -> Result<(), HarnessError> {
    linker
        .func_wrap(
            "env",
            "println",
            |mut caller: Caller<'_, HostState>, pointer: i32, length: i32| {
                let memory = caller
                    .get_export("memory")
                    .and_then(Extern::into_memory)
                    .ok_or_else(|| anyhow::anyhow!("missing memory export"))?;
                let data = memory
                    .data(&caller)
                    .get(pointer as usize..pointer as usize + length as usize)
                    .ok_or_else(|| anyhow::anyhow!("println outside memory"))?;
                eprintln!("{}", String::from_utf8_lossy(data));
                Ok(())
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "abort",
            |_message: i32, _file: i32, line: i32, column: i32| -> anyhow::Result<()> {
                anyhow::bail!("WASM abort at {line}:{column}")
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__request_context",
            |caller: Caller<'_, HostState>| caller.data().context.len() as i32,
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__load_context",
            |mut caller: Caller<'_, HostState>, pointer: i32| -> anyhow::Result<i32> {
                let context = caller.data().context.clone();
                let memory = caller
                    .get_export("memory")
                    .and_then(Extern::into_memory)
                    .ok_or_else(|| anyhow::anyhow!("missing memory export"))?;
                memory.write(&mut caller, pointer as usize, &context)?;
                // The contract ABI declares an i32 result. The legacy JS host
                // returned `undefined`, which WebAssembly coerced to zero.
                Ok(0)
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__log",
            |mut caller: Caller<'_, HostState>, pointer: i32| -> anyhow::Result<()> {
                let message = read_arraybuffer(&mut caller, pointer)?;
                eprintln!("{}", String::from_utf8_lossy(&message));
                Ok(())
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__request_storage",
            |mut caller: Caller<'_, HostState>, pointer: i32| -> anyhow::Result<i32> {
                let key = read_arraybuffer(&mut caller, pointer)?;
                Ok(caller
                    .data()
                    .storage
                    .get(&key)
                    .map(Vec::len)
                    .unwrap_or(0)
                    .try_into()?)
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__load_storage",
            |mut caller: Caller<'_, HostState>,
             key_pointer: i32,
             output: i32|
             -> anyhow::Result<i32> {
                let key = read_arraybuffer(&mut caller, key_pointer)?;
                let value = caller.data().storage.get(&key).cloned().unwrap_or_default();
                write_arraybuffer(&mut caller, output, &value)
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__balance",
            |mut caller: Caller<'_, HostState>,
             _who: i32,
             _what: i32,
             output: i32|
             -> anyhow::Result<()> {
                write_arraybuffer(&mut caller, output, &0u128.to_le_bytes())?;
                Ok(())
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__sequence",
            |mut caller: Caller<'_, HostState>, output: i32| -> anyhow::Result<()> {
                write_arraybuffer(&mut caller, output, &0u128.to_le_bytes())?;
                Ok(())
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__fuel",
            |mut caller: Caller<'_, HostState>, output: i32| -> anyhow::Result<()> {
                write_arraybuffer(&mut caller, output, &u64::MAX.to_le_bytes())?;
                Ok(())
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__height",
            |mut caller: Caller<'_, HostState>, output: i32| -> anyhow::Result<()> {
                write_arraybuffer(&mut caller, output, &0u64.to_le_bytes())?;
                Ok(())
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap(
            "env",
            "__returndatacopy",
            |mut caller: Caller<'_, HostState>, output: i32| -> anyhow::Result<()> {
                write_arraybuffer(&mut caller, output, &[])?;
                Ok(())
            },
        )
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap("env", "__request_transaction", || -> i32 { 0 })
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap("env", "__load_transaction", |_output: i32| {})
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap("env", "__request_block", || -> i32 { 0 })
        .map_err(HarnessError::Runtime)?;
    linker
        .func_wrap("env", "__load_block", |_output: i32| {})
        .map_err(HarnessError::Runtime)?;
    for name in ["__call", "__staticcall", "__delegatecall"] {
        linker
            .func_wrap(
                "env",
                name,
                |_cellpack: i32,
                 _incoming_alkanes: i32,
                 _checkpoint: i32,
                 _start_fuel: u64|
                 -> i32 { 0 },
            )
            .map_err(HarnessError::Runtime)?;
    }
    Ok(())
}

fn memory(caller: &mut Caller<'_, HostState>) -> anyhow::Result<wasmtime::Memory> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| anyhow::anyhow!("missing memory export"))
}

fn read_arraybuffer(caller: &mut Caller<'_, HostState>, pointer: i32) -> anyhow::Result<Vec<u8>> {
    let pointer = usize::try_from(pointer)?;
    let length_offset = pointer
        .checked_sub(4)
        .ok_or_else(|| anyhow::anyhow!("arraybuffer pointer is below 4"))?;
    let memory = memory(caller)?;
    let bytes = memory.data(&*caller);
    let length: [u8; 4] = bytes
        .get(length_offset..pointer)
        .ok_or_else(|| anyhow::anyhow!("arraybuffer length is outside memory"))?
        .try_into()?;
    let length = u32::from_le_bytes(length) as usize;
    Ok(bytes
        .get(pointer..pointer + length)
        .ok_or_else(|| anyhow::anyhow!("arraybuffer data is outside memory"))?
        .to_vec())
}

fn write_arraybuffer(
    caller: &mut Caller<'_, HostState>,
    pointer: i32,
    value: &[u8],
) -> anyhow::Result<i32> {
    let pointer = usize::try_from(pointer)?;
    let length_offset = pointer
        .checked_sub(4)
        .ok_or_else(|| anyhow::anyhow!("arraybuffer pointer is below 4"))?;
    let length = u32::try_from(value.len())?.to_le_bytes();
    let memory = memory(caller)?;
    memory.write(&mut *caller, length_offset, &length)?;
    memory.write(&mut *caller, pointer, value)?;
    Ok(i32::try_from(pointer)?)
}

fn serialize_context(
    context: &ChainContext,
    opcode: u128,
    args: &[Value],
) -> Result<Vec<u8>, HarnessError> {
    let mut output = Vec::new();
    for value in [
        context.myself.0,
        context.myself.1,
        context.caller.0,
        context.caller.1,
        context.vout,
        context.incoming.len() as u128,
    ] {
        output.extend_from_slice(&value.to_le_bytes());
    }
    for incoming in &context.incoming {
        output.extend_from_slice(&incoming.block.to_le_bytes());
        output.extend_from_slice(&incoming.tx.to_le_bytes());
        output.extend_from_slice(&incoming.value.to_le_bytes());
    }
    output.extend_from_slice(&opcode.to_le_bytes());
    for arg in args {
        match arg {
            Value::U128(value) => output.extend_from_slice(&value.to_le_bytes()),
            Value::String(value) => {
                let bytes = value.as_bytes();
                if bytes.is_empty() {
                    return Err(HarnessError::InvalidResponse(
                        "string arguments must not be empty".into(),
                    ));
                }
                for chunk in bytes.chunks(16) {
                    let mut word = [0u8; 16];
                    word[..chunk.len()].copy_from_slice(chunk);
                    output.extend_from_slice(&word);
                }
            }
        }
    }
    Ok(output)
}

struct DecodedResponse {
    result: CallResult,
    storage: BTreeMap<Vec<u8>, Vec<u8>>,
}

fn decode_response(memory: &[u8], pointer: i32) -> Result<DecodedResponse, HarnessError> {
    let pointer = usize::try_from(pointer)
        .map_err(|_| HarnessError::InvalidResponse("negative response pointer".into()))?;
    let length_offset = pointer
        .checked_sub(4)
        .ok_or_else(|| HarnessError::InvalidResponse("response pointer is below 4".into()))?;
    let length_bytes: [u8; 4] = memory
        .get(length_offset..pointer)
        .ok_or_else(|| HarnessError::InvalidResponse("missing response length".into()))?
        .try_into()
        .unwrap();
    let length = u32::from_le_bytes(length_bytes) as usize;
    let response = memory
        .get(pointer..pointer + length)
        .ok_or_else(|| HarnessError::InvalidResponse("response exceeds memory".into()))?;
    let (count, mut offset) = read_u128(response, 0)?;
    let mut transfers = Vec::new();
    for _ in 0..count {
        let (block, next) = read_u128(response, offset)?;
        let (tx, next2) = read_u128(response, next)?;
        let (value, next3) = read_u128(response, next2)?;
        offset = next3;
        transfers.push(AlkaneTransfer { block, tx, value });
    }
    let (storage_count, mut offset) = read_u32(response, offset)?;
    let mut storage = BTreeMap::new();
    for _ in 0..storage_count {
        let (key_length, next) = read_u32(response, offset)?;
        let key_end = next
            .checked_add(key_length as usize)
            .ok_or_else(|| HarnessError::InvalidResponse("storage key length overflow".into()))?;
        let key = response
            .get(next..key_end)
            .ok_or_else(|| HarnessError::InvalidResponse("truncated storage key".into()))?
            .to_vec();
        let (value_length, value_start) = read_u32(response, key_end)?;
        let value_end = value_start
            .checked_add(value_length as usize)
            .ok_or_else(|| HarnessError::InvalidResponse("storage value length overflow".into()))?;
        let value = response
            .get(value_start..value_end)
            .ok_or_else(|| HarnessError::InvalidResponse("truncated storage value".into()))?
            .to_vec();
        storage.insert(key, value);
        offset = value_end;
    }
    Ok(DecodedResponse {
        result: CallResult {
            transfers,
            data: response[offset..].to_vec(),
        },
        storage,
    })
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<(u32, usize), HarnessError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| HarnessError::InvalidResponse("u32 offset overflow".into()))?;
    let word: [u8; 4] = bytes
        .get(offset..end)
        .ok_or_else(|| HarnessError::InvalidResponse("truncated u32 word".into()))?
        .try_into()
        .unwrap();
    Ok((u32::from_le_bytes(word), end))
}

fn read_u128(bytes: &[u8], offset: usize) -> Result<(u128, usize), HarnessError> {
    let end = offset + 16;
    let word: [u8; 16] = bytes
        .get(offset..end)
        .ok_or_else(|| HarnessError::InvalidResponse("truncated u128 word".into()))?
        .try_into()
        .unwrap();
    Ok((u128::from_le_bytes(word), end))
}

pub fn assert_revert<T: std::fmt::Debug>(result: Result<T, HarnessError>, message: &str) {
    let error = result.expect_err("expected contract call to revert");
    assert!(
        error.to_string().contains(message),
        "expected revert containing `{message}`, got `{error}`"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_encoding_includes_transfers_opcode_and_arguments() {
        let context = ChainContext {
            myself: (1, 2),
            caller: (3, 4),
            vout: 5,
            incoming: vec![AlkaneTransfer {
                block: 6,
                tx: 7,
                value: 8,
            }],
        };
        let encoded =
            serialize_context(&context, 9, &[Value::U128(10), Value::String("AB".into())]).unwrap();
        let words: Vec<u128> = encoded
            .chunks_exact(16)
            .map(|word| u128::from_le_bytes(word.try_into().unwrap()))
            .collect();
        assert_eq!(words, vec![1, 2, 3, 4, 5, 1, 6, 7, 8, 9, 10, 0x4241]);
    }

    #[test]
    fn response_decoding_returns_transfers_and_text() {
        let mut response = Vec::new();
        response.extend_from_slice(&1u128.to_le_bytes());
        response.extend_from_slice(&2u128.to_le_bytes());
        response.extend_from_slice(&3u128.to_le_bytes());
        response.extend_from_slice(&4u128.to_le_bytes());
        response.extend_from_slice(&0u32.to_le_bytes());
        response.extend_from_slice(b"hello");
        let mut memory = (response.len() as u32).to_le_bytes().to_vec();
        memory.extend_from_slice(&response);
        let result = decode_response(&memory, 4).unwrap().result;
        assert_eq!(result.transfers.len(), 1);
        assert_eq!(result.transfers[0].value, 4);
        assert_eq!(result.data_text(), "hello");
    }

    #[test]
    fn harness_looks_up_methods_and_reports_traps() {
        let root = std::env::temp_dir().join(format!(
            "labcoat-harness-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let wasm = wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (data (i32.const 0)
                    "\16\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\68\69")
                (func (export "__execute") (result i32) i32.const 4)
            )"#,
        )
        .unwrap();
        let wasm_path = root.join("Example.wasm");
        let abi_path = root.join("Example.abi.json");
        std::fs::write(&wasm_path, wasm).unwrap();
        std::fs::write(
            &abi_path,
            r#"{"contract":"Example","methods":[{"name":"greet","opcode":1,"params":[],"returns":"String"}]}"#,
        )
        .unwrap();

        let mut harness = ContractHarness::from_files(&wasm_path, &abi_path).unwrap();
        let result = harness.call_method("greet", &[]).unwrap();
        assert_eq!(result.data_text(), "hi");
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "hi");
        assert!(matches!(
            harness.call_method("Missing", &[]),
            Err(HarnessError::UnknownMethod(_))
        ));

        let trap = wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (func (export "__execute") (result i32) unreachable)
            )"#,
        )
        .unwrap();
        std::fs::write(&wasm_path, trap).unwrap();
        let mut harness = ContractHarness::from_files(&wasm_path, &abi_path).unwrap();
        assert_revert(harness.call_method("greet", &[]), "contract trapped");

        let expected_revert = wat::parse_str(
            r#"(module
                (import "env" "abort" (func $abort (param i32 i32 i32 i32)))
                (memory (export "memory") 1)
                (func (export "__execute") (result i32)
                    i32.const 0
                    i32.const 0
                    i32.const 12
                    i32.const 34
                    call $abort
                    i32.const 0)
            )"#,
        )
        .unwrap();
        std::fs::write(&wasm_path, expected_revert).unwrap();
        let mut harness = ContractHarness::from_files(&wasm_path, &abi_path).unwrap();
        assert_revert(harness.call_method("greet", &[]), "WASM abort at 12:34");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn storage_imports_persist_response_updates_between_calls() {
        let root = std::env::temp_dir().join(format!(
            "labcoat-storage-harness-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&root).unwrap();
        let wasm = wat::parse_str(
            r#"(module
                (import "env" "__request_storage" (func $request (param i32) (result i32)))
                (import "env" "__load_storage" (func $load (param i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "\03\00\00\00key")
                (data (i32.const 100) "\22\00\00\00")
                (data (i32.const 104) "\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\01\00\00\00\03\00\00\00key\03\00\00\00one")
                (data (i32.const 200) "\17\00\00\00")
                (data (i32.const 204) "\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00one")
                (data (i32.const 300) "\17\00\00\00")
                (data (i32.const 304) "\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00bad")
                (func (export "__execute") (result i32)
                    i32.const 4
                    call $request
                    i32.eqz
                    if (result i32)
                        i32.const 104
                    else
                        i32.const 4
                        i32.const 20
                        call $load
                        drop
                        i32.const 20
                        i32.load8_u
                        i32.const 111
                        i32.eq
                        if (result i32)
                            i32.const 204
                        else
                            i32.const 304
                        end
                    end)
            )"#,
        )
        .unwrap();
        let wasm_path = root.join("Storage.wasm");
        let abi_path = root.join("Storage.abi.json");
        std::fs::write(&wasm_path, wasm).unwrap();
        std::fs::write(
            &abi_path,
            r#"{"contract":"Storage","methods":[{"name":"tick","opcode":1,"params":[],"returns":"String"}]}"#,
        )
        .unwrap();

        let mut harness = ContractHarness::from_files(&wasm_path, &abi_path).unwrap();
        assert_eq!(harness.call_method("tick", &[]).unwrap().data_text(), "");
        assert_eq!(harness.storage_value(b"key"), Some(&b"one"[..]));
        assert_eq!(harness.call_method("tick", &[]).unwrap().data_text(), "one");
        std::fs::remove_dir_all(root).ok();
    }
}
