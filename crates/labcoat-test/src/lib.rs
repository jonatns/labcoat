//! Native host-side integration testing for Labcoat contracts.
//!
//! `labcoat test` compiles WASIp1 WebAssembly modules and points this harness
//! at the resulting artifact directory through `LABCOAT_TEST_ARTIFACT_DIR`.

/// Compatibility import for deployment scripts written against the test harness.
pub use labcoat as deploy;
pub mod e2e;

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wasmtime::{Caller, Engine, Extern, Linker, Module, Store};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
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
    /// Block height the contract reads through `height()`.
    pub height: u64,
    /// Consensus-encoded block the contract reads through `block()`. Its
    /// first 80 bytes also answer `block_header()`.
    pub block: Vec<u8>,
    /// Consensus-encoded transaction the contract reads through
    /// `transaction()`.
    pub transaction: Vec<u8>,
    /// The `tx` of the next Alkane id the chain hands out: what a contract
    /// reads through `sequence()`, and where the next clone is created
    /// (`2:<sequence>`). A call that creates clones advances it. The harness
    /// raises it past every `2:n` id already in use, as on chain, where
    /// every such id is below the sequence.
    pub sequence: u128,
}

impl ChainContext {
    pub fn deterministic() -> Self {
        Self {
            myself: (1, 1),
            caller: (2, 2),
            // What genesis leaves: `2:0` is taken.
            sequence: 1,
            ..Self::default()
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

impl HarnessError {
    /// Why the contract reverted: the message of the error it returned, or
    /// the trap that stopped it. `None` for an error that is not a revert.
    pub fn revert_reason(&self) -> Option<&str> {
        match self {
            Self::Revert(reason) => Some(reason),
            _ => None,
        }
    }
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

type AlkaneId = (u128, u128);
type Storage = BTreeMap<Vec<u8>, Vec<u8>>;

/// Nested calls deeper than this revert instead of exhausting the host stack.
const MAX_CALL_DEPTH: usize = 64;
/// The Alkanes precompile that answers `block_header()`.
const BLOCK_HEADER_PRECOMPILE: AlkaneId = (800_000_000, 0);
const BLOCK_HEADER_LEN: usize = 80;
/// Prefix of the return data of a reverted call, followed by its message.
const REVERT_SELECTOR: [u8; 4] = [0x08, 0xc3, 0x79, 0xa0];

/// What a call can change, and a failed call puts back.
#[derive(Clone, Default)]
struct Ledger {
    storage: BTreeMap<AlkaneId, Storage>,
    /// Clones, by id, and the Alkane each one takes its code from.
    clones: BTreeMap<AlkaneId, AlkaneId>,
    /// The `tx` of the next id a clone gets.
    sequence: u128,
}

/// Every contract a call can reach, and the chain it observes.
#[derive(Clone, Default)]
struct World {
    modules: BTreeMap<AlkaneId, Module>,
    ledger: Ledger,
    height: u64,
    block: Vec<u8>,
    transaction: Vec<u8>,
}

/// The Alkane a call to `target` clones, if it is a factory target: `[5, n]`
/// clones `2:n` and `[6, n]` clones `4:n`.
fn factory_template(target: AlkaneId) -> Option<AlkaneId> {
    match target {
        (5, tx) => Some((2, tx)),
        (6, tx) => Some((4, tx)),
        _ => None,
    }
}

impl World {
    /// The code that runs at `id`, following a clone to its template.
    fn module(&self, id: AlkaneId) -> Option<&Module> {
        let mut current = id;
        // A clone of a clone is a chain; it cannot be longer than this.
        for _ in 0..=self.ledger.clones.len() {
            if let Some(module) = self.modules.get(&current) {
                return Some(module);
            }
            current = *self.ledger.clones.get(&current)?;
        }
        None
    }

    /// Where a call to `target` runs. A factory target creates a clone of
    /// its template at `2:<sequence>`, with empty storage, and advances the
    /// sequence; any other target is the Alkane itself.
    fn resolve(&mut self, target: AlkaneId) -> Result<AlkaneId, String> {
        let Some(template) = factory_template(target) else {
            return Ok(target);
        };
        if self.module(template).is_none() {
            let ((block, tx), (template_block, template_tx)) = (target, template);
            return Err(format!(
                "a call to {block}:{tx} clones {template_block}:{template_tx}, \
                 and no contract is deployed there in this harness"
            ));
        }
        let clone = (2, self.ledger.sequence);
        self.ledger.sequence += 1;
        self.ledger.clones.insert(clone, template);
        self.ledger.storage.insert(clone, Storage::new());
        Ok(clone)
    }
}

/// One contract execution: a top-level call or a call one contract makes.
struct Frame {
    /// Whose code runs.
    code: AlkaneId,
    /// Whose storage it reads and writes. Differs from `code` in a
    /// delegatecall.
    account: AlkaneId,
    myself: AlkaneId,
    caller: AlkaneId,
    vout: u128,
    incoming: Vec<AlkaneTransfer>,
    /// Opcode and arguments, as 16-byte little-endian words.
    inputs: Vec<u8>,
    depth: usize,
}

struct HostState {
    wasi: WasiP1Ctx,
    context: Vec<u8>,
    engine: Engine,
    world: World,
    account: AlkaneId,
    myself: AlkaneId,
    caller: AlkaneId,
    vout: u128,
    depth: usize,
    /// What the most recent call this frame made returned.
    returndata: Vec<u8>,
    /// Set when the contract calls `abort`: whatever it returns is a revert.
    aborted: bool,
}

impl HostState {
    fn storage(&self) -> Option<&Storage> {
        self.world.ledger.storage.get(&self.account)
    }
}

pub struct ContractHarness {
    engine: Engine,
    module: Module,
    abi: ContractAbi,
    context: ChainContext,
    storage: Storage,
    /// Other contracts this one can call, by Alkane id.
    deployed: BTreeMap<AlkaneId, Module>,
    deployed_storage: BTreeMap<AlkaneId, Storage>,
    /// Clones earlier calls created, and the Alkane each takes its code from.
    clones: BTreeMap<AlkaneId, AlkaneId>,
}

fn artifact_dir() -> PathBuf {
    std::env::var_os("LABCOAT_TEST_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".labcoat/test-artifacts"))
}

impl ContractHarness {
    pub fn for_contract(name: &str) -> Result<Self, HarnessError> {
        let root = artifact_dir();
        Self::from_files(
            root.join(format!("{name}.wasm")),
            root.join(format!("{name}.abi.json")),
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
        let module = Module::from_file(&engine, wasm_path).map_err(runtime_error)?;
        let abi = serde_json::from_slice(&std::fs::read(abi_path)?)?;
        Ok(Self {
            engine,
            module,
            abi,
            context: ChainContext::deterministic(),
            storage: BTreeMap::new(),
            deployed: BTreeMap::new(),
            deployed_storage: BTreeMap::new(),
            clones: BTreeMap::new(),
        })
    }

    /// Make another built contract callable at `id`, with empty storage.
    ///
    /// The contract under test reaches it with `call`, `staticcall`, or
    /// `delegatecall`. Alkane balances are not tracked: a call hands its
    /// outgoing transfers to the callee and receives the callee's in return,
    /// and `balance()` always reads zero.
    ///
    /// A contract deployed at `2:n` or `4:n` is also a template: a call to
    /// `[5, n]` or `[6, n]` creates a clone of it at `2:<sequence>`, with the
    /// template's code and storage of its own, and runs there. The clone
    /// stays on the harness, so a later call can target it by its id.
    pub fn deploy(&mut self, id: (u128, u128), name: &str) -> Result<&mut Self, HarnessError> {
        self.deploy_file(id, artifact_dir().join(format!("{name}.wasm")))
    }

    /// Make the contract compiled at `wasm_path` callable at `id`.
    pub fn deploy_file(
        &mut self,
        id: (u128, u128),
        wasm_path: impl AsRef<Path>,
    ) -> Result<&mut Self, HarnessError> {
        let wasm_path = wasm_path.as_ref();
        if !wasm_path.exists() {
            return Err(HarnessError::ArtifactMissing(wasm_path.to_path_buf()));
        }
        let module = Module::from_file(&self.engine, wasm_path).map_err(runtime_error)?;
        self.deployed.insert(id, module);
        self.deployed_storage.insert(id, BTreeMap::new());
        self.clones.remove(&id);
        Ok(self)
    }

    /// Return a raw storage value of a contract added with [`Self::deploy`],
    /// or of a clone a call created.
    pub fn storage_value_of(&self, id: (u128, u128), key: impl AsRef<[u8]>) -> Option<&[u8]> {
        self.deployed_storage
            .get(&id)?
            .get(key.as_ref())
            .map(Vec::as_slice)
    }

    /// Seed or replace a raw storage value of a contract added with
    /// [`Self::deploy`], or of a clone a call created.
    pub fn set_storage_of(
        &mut self,
        id: (u128, u128),
        key: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
    ) -> &mut Self {
        self.deployed_storage
            .entry(id)
            .or_default()
            .insert(key.as_ref().to_vec(), value.as_ref().to_vec());
        self
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

    /// Decode a 16-byte little-endian unsigned integer from contract storage.
    ///
    /// Returns `None` when the key is missing or its value is not exactly 16 bytes.
    pub fn storage_u128(&self, key: impl AsRef<[u8]>) -> Option<u128> {
        let bytes: [u8; 16] = self.storage_value(key)?.try_into().ok()?;
        Some(u128::from_le_bytes(bytes))
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
        let myself = self.context.myself;
        // The contract under test is deployed last, so it wins its own id.
        let mut world = World {
            modules: self.deployed.clone(),
            ledger: Ledger {
                storage: self.deployed_storage.clone(),
                clones: self.clones.clone(),
                sequence: self.sequence(),
            },
            height: self.context.height,
            block: self.context.block.clone(),
            transaction: self.context.transaction.clone(),
        };
        world.modules.insert(myself, self.module.clone());
        world.ledger.storage.insert(myself, self.storage.clone());
        // A context whose `myself` is a factory target is a transaction that
        // clones a deployed template and calls the clone.
        let target = world.resolve(myself).map_err(HarnessError::Revert)?;

        let frame = Frame {
            code: target,
            account: target,
            myself: target,
            caller: self.context.caller,
            vout: self.context.vout,
            incoming: self.context.incoming.clone(),
            inputs: serialize_inputs(opcode, args)?,
            depth: 0,
        };
        let (world, result) = run_frame(&self.engine, world, frame);
        // A failed call changes nothing: the world it ran in is dropped.
        let decoded = result?;
        let mut ledger = world.ledger;
        ledger
            .storage
            .entry(target)
            .or_default()
            .extend(decoded.storage);
        self.storage = ledger.storage.remove(&myself).unwrap_or_default();
        self.deployed_storage = ledger.storage;
        self.clones = ledger.clones;
        self.context.sequence = ledger.sequence;
        Ok(decoded.result)
    }

    /// The sequence the next call starts from: the context's, raised past
    /// every `2:n` id this harness already uses.
    fn sequence(&self) -> u128 {
        self.deployed
            .keys()
            .chain(self.clones.keys())
            .chain([&self.context.myself])
            .filter(|(block, _)| *block == 2)
            .map(|(_, tx)| tx.saturating_add(1))
            .chain([self.context.sequence])
            .max()
            .unwrap_or_default()
    }
}

/// Run one frame to completion. The world comes back whether or not the
/// frame succeeded; the caller decides what to keep.
fn run_frame(
    engine: &Engine,
    world: World,
    frame: Frame,
) -> (World, Result<DecodedResponse, HarnessError>) {
    let Some(module) = world.module(frame.code).cloned() else {
        let (block, tx) = frame.code;
        return (
            world,
            Err(HarnessError::Revert(format!(
                "no contract is deployed at {block}:{tx} in this harness"
            ))),
        );
    };
    let state = HostState {
        wasi: WasiCtxBuilder::new().build_p1(),
        context: serialize_frame_context(&frame),
        engine: engine.clone(),
        world,
        account: frame.account,
        myself: frame.myself,
        caller: frame.caller,
        vout: frame.vout,
        depth: frame.depth,
        returndata: Vec::new(),
        aborted: false,
    };
    let mut store = Store::new(engine, state);
    let result = execute(&mut store, &module);
    (store.into_data().world, result)
}

fn execute(store: &mut Store<HostState>, module: &Module) -> Result<DecodedResponse, HarnessError> {
    let mut linker: Linker<HostState> = Linker::new(store.engine());
    p1::add_to_linker_sync(&mut linker, |state| &mut state.wasi).map_err(runtime_error)?;
    add_alkanes_imports(&mut linker)?;
    let instance = linker
        .instantiate(&mut *store, module)
        .map_err(runtime_error)?;
    if let Ok(initialize) = instance.get_typed_func::<(), ()>(&mut *store, "_initialize") {
        initialize
            .call(&mut *store, ())
            .map_err(|e| HarnessError::Revert(format!("{e:#}")))?;
    }
    let execute = instance
        .get_typed_func::<(), i32>(&mut *store, "__execute")
        .map_err(runtime_error)?;
    let pointer = execute
        .call(&mut *store, ())
        .map_err(|e| HarnessError::Revert(format!("{e:#}")))?;
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| HarnessError::InvalidResponse("missing memory export".into()))?;
    let response = decode_response(memory.data(&*store), pointer);
    if store.data().aborted {
        // alkanes-runtime reports an error by calling `abort` and then
        // returning a response whose data is the revert selector followed by
        // the message.
        let reason = response
            .ok()
            .and_then(|decoded| {
                let message = decoded.result.data.strip_prefix(&REVERT_SELECTOR)?;
                Some(String::from_utf8_lossy(message).into_owned())
            })
            .unwrap_or_else(|| "the contract aborted without a message".into());
        return Err(HarnessError::Revert(reason));
    }
    response
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CallKind {
    Call,
    Static,
    Delegate,
}

/// A call one contract makes to another. Returns the length of the return
/// data, negated when the callee reverted, as the Alkanes host does.
fn extcall(
    caller: &mut Caller<'_, HostState>,
    kind: CallKind,
    cellpack: i32,
    outgoing: i32,
    checkpoint: i32,
) -> wasmtime::Result<i32> {
    let cellpack = read_arraybuffer(caller, cellpack)?;
    let outgoing = read_arraybuffer(caller, outgoing)?;
    let checkpoint = read_arraybuffer(caller, checkpoint)?;
    let words: Vec<u128> = cellpack
        .as_chunks::<16>()
        .0
        .iter()
        .map(|word| u128::from_le_bytes(*word))
        .collect();
    let [block, tx, ..] = words[..] else {
        wasmtime::bail!("a call's cellpack has no target");
    };
    let target = (block, tx);
    let (transfers, _) = read_transfers(&outgoing, 0).map_err(wasmtime::Error::msg)?;
    let (pending, _) = read_storage(&checkpoint, 0).map_err(wasmtime::Error::msg)?;

    let state = caller.data_mut();
    let engine = state.engine.clone();
    let mut world = std::mem::take(&mut state.world);
    let outcome: Result<CallResult, String> = if state.depth + 1 >= MAX_CALL_DEPTH {
        Err(format!("calls nested deeper than {MAX_CALL_DEPTH}"))
    } else if target == BLOCK_HEADER_PRECOMPILE {
        match world.block.get(..BLOCK_HEADER_LEN) {
            Some(header) => Ok(CallResult {
                transfers: Vec::new(),
                data: header.to_vec(),
            }),
            None => Err("no block is set; assign ChainContext::block".into()),
        }
    } else {
        let before = world.ledger.clone();
        // The callee sees what its caller has written so far.
        world
            .ledger
            .storage
            .entry(state.account)
            .or_default()
            .extend(pending);
        // A factory target creates the clone this call runs at. A
        // delegatecall still creates it, but runs its code as the caller.
        match world.resolve(target) {
            Err(message) => {
                world.ledger = before;
                Err(message)
            }
            Ok(target) => {
                let frame = Frame {
                    code: target,
                    account: if kind == CallKind::Delegate {
                        state.account
                    } else {
                        target
                    },
                    myself: if kind == CallKind::Delegate {
                        state.myself
                    } else {
                        target
                    },
                    caller: if kind == CallKind::Delegate {
                        state.caller
                    } else {
                        state.myself
                    },
                    vout: state.vout,
                    incoming: transfers,
                    inputs: cellpack[32..].to_vec(),
                    depth: state.depth + 1,
                };
                let account = frame.account;
                let (returned, result) = run_frame(&engine, world, frame);
                world = returned;
                match result {
                    Ok(decoded) if kind == CallKind::Static => {
                        world.ledger = before;
                        Ok(decoded.result)
                    }
                    Ok(decoded) => {
                        world
                            .ledger
                            .storage
                            .entry(account)
                            .or_default()
                            .extend(decoded.storage);
                        Ok(decoded.result)
                    }
                    Err(error) => {
                        // Nothing the callee did survives: no storage, no
                        // clone, and the sequence is where it was.
                        world.ledger = before;
                        Err(match error {
                            // The text the Alkanes indexer hands the caller.
                            HarnessError::Revert(reason) => format!("ALKANES: revert: {reason}"),
                            other => other.to_string(),
                        })
                    }
                }
            }
        }
    };

    let state = caller.data_mut();
    state.world = world;
    let (reverted, response) = match outcome {
        Ok(response) => (false, response),
        Err(message) => {
            let mut data = REVERT_SELECTOR.to_vec();
            data.extend_from_slice(message.as_bytes());
            (
                true,
                CallResult {
                    transfers: Vec::new(),
                    data,
                },
            )
        }
    };
    state.returndata = serialize_call_response(&response);
    let length = i32::try_from(state.returndata.len())?;
    Ok(if reverted { -length } else { length })
}

fn runtime_error(error: wasmtime::Error) -> HarnessError {
    HarnessError::Runtime(error.into())
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
                    .ok_or_else(|| wasmtime::format_err!("missing memory export"))?;
                let data = memory
                    .data(&caller)
                    .get(pointer as usize..pointer as usize + length as usize)
                    .ok_or_else(|| wasmtime::format_err!("println outside memory"))?;
                eprintln!("{}", String::from_utf8_lossy(data));
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "abort",
            // As on chain, `abort` marks the call failed and returns; the
            // reason is in the response the contract goes on to return.
            |mut caller: Caller<'_, HostState>,
             _message: i32,
             _file: i32,
             _line: i32,
             _column: i32| {
                caller.data_mut().aborted = true;
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__request_context",
            |caller: Caller<'_, HostState>| caller.data().context.len() as i32,
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__load_context",
            |mut caller: Caller<'_, HostState>, pointer: i32| -> wasmtime::Result<i32> {
                let context = caller.data().context.clone();
                let memory = caller
                    .get_export("memory")
                    .and_then(Extern::into_memory)
                    .ok_or_else(|| wasmtime::format_err!("missing memory export"))?;
                memory.write(&mut caller, pointer as usize, &context)?;
                // The contract ABI declares an i32 result.
                Ok(0)
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__log",
            |mut caller: Caller<'_, HostState>, pointer: i32| -> wasmtime::Result<()> {
                let message = read_arraybuffer(&mut caller, pointer)?;
                eprintln!("{}", String::from_utf8_lossy(&message));
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__request_storage",
            |mut caller: Caller<'_, HostState>, pointer: i32| -> wasmtime::Result<i32> {
                let key = read_arraybuffer(&mut caller, pointer)?;
                Ok(caller
                    .data()
                    .storage()
                    .and_then(|storage| storage.get(&key))
                    .map(Vec::len)
                    .unwrap_or(0)
                    .try_into()?)
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__load_storage",
            |mut caller: Caller<'_, HostState>,
             key_pointer: i32,
             output: i32|
             -> wasmtime::Result<i32> {
                let key = read_arraybuffer(&mut caller, key_pointer)?;
                let value = caller
                    .data()
                    .storage()
                    .and_then(|storage| storage.get(&key))
                    .cloned()
                    .unwrap_or_default();
                write_arraybuffer(&mut caller, output, &value)
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__balance",
            |mut caller: Caller<'_, HostState>,
             _who: i32,
             _what: i32,
             output: i32|
             -> wasmtime::Result<()> {
                write_arraybuffer(&mut caller, output, &0u128.to_le_bytes())?;
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__sequence",
            |mut caller: Caller<'_, HostState>, output: i32| -> wasmtime::Result<()> {
                let sequence = caller.data().world.ledger.sequence;
                write_arraybuffer(&mut caller, output, &sequence.to_le_bytes())?;
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__fuel",
            |mut caller: Caller<'_, HostState>, output: i32| -> wasmtime::Result<()> {
                write_arraybuffer(&mut caller, output, &u64::MAX.to_le_bytes())?;
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__height",
            |mut caller: Caller<'_, HostState>, output: i32| -> wasmtime::Result<()> {
                let height = caller.data().world.height;
                write_arraybuffer(&mut caller, output, &height.to_le_bytes())?;
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__returndatacopy",
            |mut caller: Caller<'_, HostState>, output: i32| -> wasmtime::Result<()> {
                let returndata = caller.data().returndata.clone();
                write_arraybuffer(&mut caller, output, &returndata)?;
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__request_transaction",
            |caller: Caller<'_, HostState>| caller.data().world.transaction.len() as i32,
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__load_transaction",
            |mut caller: Caller<'_, HostState>, output: i32| -> wasmtime::Result<()> {
                let transaction = caller.data().world.transaction.clone();
                memory(&mut caller)?.write(&mut caller, usize::try_from(output)?, &transaction)?;
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    linker
        .func_wrap("env", "__request_block", |caller: Caller<'_, HostState>| {
            caller.data().world.block.len() as i32
        })
        .map_err(runtime_error)?;
    linker
        .func_wrap(
            "env",
            "__load_block",
            |mut caller: Caller<'_, HostState>, output: i32| -> wasmtime::Result<()> {
                let block = caller.data().world.block.clone();
                memory(&mut caller)?.write(&mut caller, usize::try_from(output)?, &block)?;
                Ok(())
            },
        )
        .map_err(runtime_error)?;
    for (name, kind) in [
        ("__call", CallKind::Call),
        ("__staticcall", CallKind::Static),
        ("__delegatecall", CallKind::Delegate),
    ] {
        linker
            .func_wrap(
                "env",
                name,
                move |mut caller: Caller<'_, HostState>,
                      cellpack: i32,
                      outgoing: i32,
                      checkpoint: i32,
                      _start_fuel: u64|
                      -> wasmtime::Result<i32> {
                    extcall(&mut caller, kind, cellpack, outgoing, checkpoint)
                },
            )
            .map_err(runtime_error)?;
    }
    Ok(())
}

fn memory(caller: &mut Caller<'_, HostState>) -> wasmtime::Result<wasmtime::Memory> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmtime::format_err!("missing memory export"))
}

fn read_arraybuffer(caller: &mut Caller<'_, HostState>, pointer: i32) -> wasmtime::Result<Vec<u8>> {
    let pointer = usize::try_from(pointer)?;
    let length_offset = pointer
        .checked_sub(4)
        .ok_or_else(|| wasmtime::format_err!("arraybuffer pointer is below 4"))?;
    let memory = memory(caller)?;
    let bytes = memory.data(&*caller);
    let length: [u8; 4] = bytes
        .get(length_offset..pointer)
        .ok_or_else(|| wasmtime::format_err!("arraybuffer length is outside memory"))?
        .try_into()?;
    let length = u32::from_le_bytes(length) as usize;
    Ok(bytes
        .get(pointer..pointer + length)
        .ok_or_else(|| wasmtime::format_err!("arraybuffer data is outside memory"))?
        .to_vec())
}

fn write_arraybuffer(
    caller: &mut Caller<'_, HostState>,
    pointer: i32,
    value: &[u8],
) -> wasmtime::Result<i32> {
    let pointer = usize::try_from(pointer)?;
    let length_offset = pointer
        .checked_sub(4)
        .ok_or_else(|| wasmtime::format_err!("arraybuffer pointer is below 4"))?;
    let length = u32::try_from(value.len())?.to_le_bytes();
    let memory = memory(caller)?;
    memory.write(&mut *caller, length_offset, &length)?;
    memory.write(&mut *caller, pointer, value)?;
    Ok(i32::try_from(pointer)?)
}

fn serialize_inputs(opcode: u128, args: &[Value]) -> Result<Vec<u8>, HarnessError> {
    let mut output = opcode.to_le_bytes().to_vec();
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

fn write_transfers(output: &mut Vec<u8>, transfers: &[AlkaneTransfer]) {
    output.extend_from_slice(&(transfers.len() as u128).to_le_bytes());
    for transfer in transfers {
        output.extend_from_slice(&transfer.block.to_le_bytes());
        output.extend_from_slice(&transfer.tx.to_le_bytes());
        output.extend_from_slice(&transfer.value.to_le_bytes());
    }
}

/// The context a contract loads: who it is, who called, what arrived, and
/// the opcode and arguments.
fn serialize_frame_context(frame: &Frame) -> Vec<u8> {
    let mut output = Vec::new();
    for value in [
        frame.myself.0,
        frame.myself.1,
        frame.caller.0,
        frame.caller.1,
        frame.vout,
    ] {
        output.extend_from_slice(&value.to_le_bytes());
    }
    write_transfers(&mut output, &frame.incoming);
    output.extend_from_slice(&frame.inputs);
    output
}

/// What a caller reads back from a call: transfers, then data.
fn serialize_call_response(response: &CallResult) -> Vec<u8> {
    let mut output = Vec::new();
    write_transfers(&mut output, &response.transfers);
    output.extend_from_slice(&response.data);
    output
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
    let (transfers, offset) = read_transfers(response, 0)?;
    let (storage, offset) = read_storage(response, offset)?;
    Ok(DecodedResponse {
        result: CallResult {
            transfers,
            data: response[offset..].to_vec(),
        },
        storage,
    })
}

fn read_transfers(
    bytes: &[u8],
    offset: usize,
) -> Result<(Vec<AlkaneTransfer>, usize), HarnessError> {
    let (count, mut offset) = read_u128(bytes, offset)?;
    let mut transfers = Vec::new();
    for _ in 0..count {
        let (block, next) = read_u128(bytes, offset)?;
        let (tx, next) = read_u128(bytes, next)?;
        let (value, next) = read_u128(bytes, next)?;
        offset = next;
        transfers.push(AlkaneTransfer { block, tx, value });
    }
    Ok((transfers, offset))
}

fn read_storage(bytes: &[u8], offset: usize) -> Result<(Storage, usize), HarnessError> {
    let (count, mut offset) = read_u32(bytes, offset)?;
    let mut storage = BTreeMap::new();
    for _ in 0..count {
        let (key_length, next) = read_u32(bytes, offset)?;
        let key_end = next
            .checked_add(key_length as usize)
            .ok_or_else(|| HarnessError::InvalidResponse("storage key length overflow".into()))?;
        let key = bytes
            .get(next..key_end)
            .ok_or_else(|| HarnessError::InvalidResponse("truncated storage key".into()))?
            .to_vec();
        let (value_length, value_start) = read_u32(bytes, key_end)?;
        let value_end = value_start
            .checked_add(value_length as usize)
            .ok_or_else(|| HarnessError::InvalidResponse("storage value length overflow".into()))?;
        let value = bytes
            .get(value_start..value_end)
            .ok_or_else(|| HarnessError::InvalidResponse("truncated storage value".into()))?
            .to_vec();
        storage.insert(key, value);
        offset = value_end;
    }
    Ok((storage, offset))
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

pub fn assert_revert<T: std::fmt::Debug>(result: Result<T, HarnessError>) {
    result.expect_err("expected contract call to revert");
}

/// Assert that a call reverted and that its reason contains `expected`, for
/// example the message of the error the contract returned.
pub fn assert_revert_with<T: std::fmt::Debug>(result: Result<T, HarnessError>, expected: &str) {
    let error = result.expect_err("expected contract call to revert");
    let Some(reason) = error.revert_reason() else {
        panic!("expected contract call to revert, but it failed with: {error}");
    };
    assert!(
        reason.contains(expected),
        "expected a revert containing `{expected}`, but the reason was `{reason}`"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_encoding_includes_transfers_opcode_and_arguments() {
        let frame = Frame {
            code: (1, 2),
            account: (1, 2),
            myself: (1, 2),
            caller: (3, 4),
            vout: 5,
            incoming: vec![AlkaneTransfer {
                block: 6,
                tx: 7,
                value: 8,
            }],
            inputs: serialize_inputs(9, &[Value::U128(10), Value::String("AB".into())]).unwrap(),
            depth: 0,
        };
        assert_eq!(
            words(&serialize_frame_context(&frame)),
            vec![1, 2, 3, 4, 5, 1, 6, 7, 8, 9, 10, 0x4241]
        );
    }

    fn words(bytes: &[u8]) -> Vec<u128> {
        bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|word| u128::from_le_bytes(*word))
            .collect()
    }

    /// A scratch directory of compiled test contracts, removed on drop.
    struct Contracts(PathBuf);

    impl Contracts {
        fn new(test: &str) -> Self {
            let root = std::env::temp_dir().join(format!("labcoat-{test}-{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn wasm(&self, name: &str, wat: &str) -> PathBuf {
            let path = self.0.join(format!("{name}.wasm"));
            std::fs::write(&path, wat::parse_str(wat).unwrap()).unwrap();
            path
        }

        fn harness(&self, name: &str, wat: &str) -> ContractHarness {
            let abi = self.0.join(format!("{name}.abi.json"));
            std::fs::write(&abi, r#"{"contract":"Test","methods":[]}"#).unwrap();
            ContractHarness::from_files(self.wasm(name, wat), abi).unwrap()
        }
    }

    impl Drop for Contracts {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn wat_bytes(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("\\{byte:02x}")).collect()
    }

    /// Returns its own context as data.
    const ECHO_CONTEXT: &str = r#"(module
        (import "env" "__request_context" (func $request (result i32)))
        (import "env" "__load_context" (func $load (param i32) (result i32)))
        (memory (export "memory") 1)
        (func (export "__execute") (result i32)
            (i32.store (i32.const 100) (i32.add (i32.const 20) (call $request)))
            (drop (call $load (i32.const 124)))
            (i32.const 104)))"#;

    /// Stores `hit = yes` and returns `pong`.
    const WRITER: &str = r#"(module
        (memory (export "memory") 1)
        (data (i32.const 100) "\26\00\00\00")
        (data (i32.const 120) "\01\00\00\00\03\00\00\00hit\03\00\00\00yespong")
        (func (export "__execute") (result i32) (i32.const 104)))"#;

    const TRAP: &str = r#"(module
        (memory (export "memory") 1)
        (func (export "__execute") (result i32) unreachable))"#;

    /// Calls `target` with opcode 7 through the host import `import` and
    /// returns what the callee returned, or `no` if it reverted.
    fn caller_wat(import: &str, target: AlkaneId) -> String {
        let mut cellpack = Vec::new();
        for word in [target.0, target.1, 7] {
            cellpack.extend_from_slice(&word.to_le_bytes());
        }
        format!(
            r#"(module
                (import "env" "{import}" (func $call (param i32 i32 i32 i64) (result i32)))
                (import "env" "__returndatacopy" (func $copy (param i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "\30\00\00\00{cellpack}")
                (data (i32.const 60) "\10\00\00\00")
                (data (i32.const 90) "\04\00\00\00")
                (data (i32.const 400) "\16\00\00\00")
                (data (i32.const 424) "no")
                (func (export "__execute") (result i32)
                    (local $length i32)
                    (local.set $length
                        (call $call (i32.const 4) (i32.const 64) (i32.const 94) (i64.const 0)))
                    (if (result i32) (i32.lt_s (local.get $length) (i32.const 0))
                        (then (i32.const 404))
                        (else
                            (call $copy (i32.const 220))
                            (i32.store (i32.const 1000) (i32.add (i32.const 4) (local.get $length)))
                            (memory.copy
                                (i32.const 1024)
                                (i32.const 236)
                                (i32.sub (local.get $length) (i32.const 16)))
                            (i32.const 1004)))))"#,
            cellpack = wat_bytes(&cellpack)
        )
    }

    const CALLEE: AlkaneId = (2, 9);

    /// Calls the Alkane named by its opcode and first argument with opcode
    /// 7, and returns what the callee returned, or `no` if it reverted.
    fn forwarder_wat() -> String {
        caller_wat("__call", (0, 0))
            .replace(
                r#"(memory (export "memory") 1)"#,
                r#"(import "env" "__load_context" (func $load (param i32) (result i32)))
                (memory (export "memory") 1)"#,
            )
            .replace(
                "(local.set $length",
                "(drop (call $load (i32.const 2000)))
                (memory.copy (i32.const 4) (i32.const 2096) (i32.const 32))
                (local.set $length",
            )
    }

    /// Like [`caller_wat`], but returns the callee's revert data instead of
    /// `no`.
    fn revert_reader_wat(target: AlkaneId) -> String {
        caller_wat("__call", target).replace(
            "(then (i32.const 404))",
            "(then
                (local.set $length (i32.sub (i32.const 0) (local.get $length)))
                (call $copy (i32.const 220))
                (i32.store (i32.const 1000) (i32.add (i32.const 4) (local.get $length)))
                (memory.copy
                    (i32.const 1024)
                    (i32.const 236)
                    (i32.sub (local.get $length) (i32.const 16)))
                (i32.const 1004))",
        )
    }

    /// Stores `init = 1` and returns `ok`; once that is stored, reverts with
    /// `already initialized` the way alkanes-runtime reports an error.
    const INIT_ONCE: &str = r#"(module
        (import "env" "abort" (func $abort (param i32 i32 i32 i32)))
        (import "env" "__request_storage" (func $request (param i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "\04\00\00\00init")
        (data (i32.const 100) "\23\00\00\00")
        (data (i32.const 120) "\01\00\00\00\04\00\00\00init\01\00\00\001ok")
        (data (i32.const 200) "\2b\00\00\00")
        (data (i32.const 224) "\08\c3\79\a0already initialized")
        (func (export "__execute") (result i32)
            (if (result i32) (call $request (i32.const 4))
                (then
                    (call $abort (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))
                    (i32.const 204))
                (else (i32.const 104)))))"#;

    /// Returns the sequence it reads.
    const SEQUENCE: &str = r#"(module
        (import "env" "__sequence" (func $sequence (param i32)))
        (memory (export "memory") 1)
        (func (export "__execute") (result i32)
            (call $sequence (i32.const 24))
            (i32.store (i32.const 100) (i32.const 36))
            (memory.copy (i32.const 124) (i32.const 24) (i32.const 16))
            (i32.const 104)))"#;

    const TEMPLATE: AlkaneId = (2, 9);
    const RESERVED_TEMPLATE: AlkaneId = (4, 9);

    #[test]
    fn a_call_to_a_factory_target_runs_a_clone_at_the_next_sequence() {
        let contracts = Contracts::new("clone");
        let mut harness = contracts.harness("factory", &forwarder_wat());
        let echo = contracts.wasm("echo", ECHO_CONTEXT);
        harness.deploy_file(TEMPLATE, &echo).unwrap();
        harness.deploy_file(RESERVED_TEMPLATE, &echo).unwrap();

        // The sequence starts past the highest `2:n` id in use.
        let context = harness.call_opcode(5, &[Value::U128(9)]).unwrap().data;
        assert_eq!(words(&context), vec![2, 10, 1, 1, 0, 0, 7]);
        assert_eq!(harness.context().sequence, 11);

        harness.context_mut().sequence = 100;
        let context = harness.call_opcode(6, &[Value::U128(9)]).unwrap().data;
        assert_eq!(words(&context), vec![2, 100, 1, 1, 0, 0, 7]);
        assert_eq!(harness.context().sequence, 101);
    }

    #[test]
    fn each_clone_has_its_own_storage_and_stays_callable() {
        let contracts = Contracts::new("clones");
        let mut harness = contracts.harness("factory", &forwarder_wat());
        harness
            .deploy_file(TEMPLATE, contracts.wasm("init", INIT_ONCE))
            .unwrap();
        harness
            .deploy_file(RESERVED_TEMPLATE, contracts.wasm("init", INIT_ONCE))
            .unwrap();
        harness.context_mut().sequence = 100;

        // A second clone would revert if it shared the first one's storage.
        for target in [5, 6, 5] {
            let result = harness.call_opcode(target, &[Value::U128(9)]).unwrap();
            assert_eq!(result.data_text(), "ok");
        }
        assert_eq!(harness.context().sequence, 103);
        for clone in 100..103 {
            assert_eq!(
                harness.storage_value_of((2, clone), b"init"),
                Some(&b"1"[..])
            );
        }
        assert_eq!(harness.storage_value_of(TEMPLATE, b"init"), None);
        assert_eq!(harness.storage_value_of(RESERVED_TEMPLATE, b"init"), None);
        assert_eq!(harness.storage_value_of((2, 103), b"init"), None);

        // A later call reaches a clone by its id, and finds what it stored.
        let again = harness.call_opcode(2, &[Value::U128(101)]).unwrap();
        assert_eq!(again.data_text(), "no");
        assert_eq!(harness.context().sequence, 103);
    }

    #[test]
    fn a_contract_reads_the_sequence_its_clones_advanced() {
        let contracts = Contracts::new("sequence");
        let mut harness = contracts.harness("factory", &forwarder_wat());
        harness
            .deploy_file(TEMPLATE, contracts.wasm("sequence", SEQUENCE))
            .unwrap();
        harness.context_mut().sequence = 100;

        // A clone is created before it runs, so it reads the id after its own.
        for expected in [101u128, 102] {
            let sequence = harness.call_opcode(5, &[Value::U128(9)]).unwrap().data;
            assert_eq!(sequence, expected.to_le_bytes());
        }
        let sequence = harness.call_opcode(2, &[Value::U128(9)]).unwrap().data;
        assert_eq!(sequence, 102u128.to_le_bytes());
    }

    #[test]
    fn a_reverted_clone_leaves_nothing_and_keeps_the_sequence() {
        let contracts = Contracts::new("clone-revert");
        let mut harness = contracts.harness("factory", &forwarder_wat());
        harness.context_mut().sequence = 100;

        // No template to clone.
        assert_eq!(
            harness
                .call_opcode(5, &[Value::U128(9)])
                .unwrap()
                .data_text(),
            "no"
        );
        assert_eq!(harness.context().sequence, 100);

        // The clone itself reverts.
        harness
            .deploy_file(TEMPLATE, contracts.wasm("trap", TRAP))
            .unwrap();
        assert_eq!(
            harness
                .call_opcode(5, &[Value::U128(9)])
                .unwrap()
                .data_text(),
            "no"
        );
        assert_eq!(harness.context().sequence, 100);

        // The contract that made the clone reverts after it.
        let wat = caller_wat("__call", (5, 9)).replace(
            "(call $copy (i32.const 220))",
            "(call $copy (i32.const 220)) unreachable",
        );
        let mut harness = contracts.harness("failing-factory", &wat);
        harness
            .deploy_file(TEMPLATE, contracts.wasm("writer", WRITER))
            .unwrap();
        harness.context_mut().sequence = 100;
        assert_revert(harness.call_opcode(1, &[]));
        assert_eq!(harness.context().sequence, 100);
        assert_eq!(harness.storage_value_of((2, 100), b"hit"), None);

        // The next clone takes the id none of them kept.
        let mut harness = contracts.harness("factory", &forwarder_wat());
        harness
            .deploy_file(TEMPLATE, contracts.wasm("writer", WRITER))
            .unwrap();
        harness.context_mut().sequence = 100;
        harness
            .deploy_file((4, 1), contracts.wasm("trap", TRAP))
            .unwrap();
        assert_eq!(
            harness
                .call_opcode(6, &[Value::U128(1)])
                .unwrap()
                .data_text(),
            "no"
        );
        assert_eq!(
            harness
                .call_opcode(5, &[Value::U128(9)])
                .unwrap()
                .data_text(),
            "pong"
        );
        assert_eq!(
            harness.storage_value_of((2, 100), b"hit"),
            Some(&b"yes"[..])
        );
    }

    #[test]
    fn a_static_call_to_a_factory_target_keeps_no_clone() {
        let contracts = Contracts::new("clone-static");
        let mut harness = contracts.harness("caller", &caller_wat("__staticcall", (5, 9)));
        harness
            .deploy_file(TEMPLATE, contracts.wasm("echo", ECHO_CONTEXT))
            .unwrap();
        harness.context_mut().sequence = 100;
        let context = harness.call_opcode(1, &[]).unwrap().data;
        assert_eq!(words(&context), vec![2, 100, 1, 1, 0, 0, 7]);
        assert_eq!(harness.context().sequence, 100);
    }

    #[test]
    fn a_top_level_call_to_a_factory_target_runs_a_clone() {
        let contracts = Contracts::new("clone-top");
        let mut harness = contracts.harness("unused", TRAP);
        harness
            .deploy_file(TEMPLATE, contracts.wasm("init", INIT_ONCE))
            .unwrap();
        harness.context_mut().myself = (5, 9);
        harness.context_mut().sequence = 100;
        for clone in [100, 101] {
            assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "ok");
            assert_eq!(
                harness.storage_value_of((2, clone), b"init"),
                Some(&b"1"[..])
            );
        }
        assert_eq!(harness.context().sequence, 102);

        harness.context_mut().myself = (5, 8);
        assert_revert_with(harness.call_opcode(1, &[]), "clones 2:8");
        assert_eq!(harness.context().sequence, 102);
    }

    #[test]
    fn a_top_level_revert_reports_the_reason_the_contract_gave() {
        let contracts = Contracts::new("reason");
        let mut harness = contracts.harness("init", INIT_ONCE);
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "ok");

        let error = harness.call_opcode(1, &[]).unwrap_err();
        assert!(matches!(&error, HarnessError::Revert(reason) if reason == "already initialized"));
        assert_eq!(error.revert_reason(), Some("already initialized"));
        assert_revert(harness.call_opcode(1, &[]));
        assert_revert_with(harness.call_opcode(1, &[]), "already initialized");
        assert_eq!(harness.storage_value(b"init"), Some(&b"1"[..]));

        // An abort with no response to read is still a revert.
        let mut harness = contracts.harness(
            "abort",
            r#"(module
                (import "env" "abort" (func $abort (param i32 i32 i32 i32)))
                (memory (export "memory") 1)
                (func (export "__execute") (result i32)
                    (call $abort (i32.const 0) (i32.const 0) (i32.const 12) (i32.const 34))
                    (i32.const 0)))"#,
        );
        assert!(harness
            .call_opcode(1, &[])
            .unwrap_err()
            .revert_reason()
            .is_some());
        assert!(HarnessError::UnknownMethod("x".into())
            .revert_reason()
            .is_none());
    }

    #[test]
    fn a_caller_reads_the_reason_its_callee_reverted_with() {
        let contracts = Contracts::new("nested-reason");
        let mut harness = contracts.harness("caller", &revert_reader_wat(CALLEE));
        harness
            .deploy_file(CALLEE, contracts.wasm("init", INIT_ONCE))
            .unwrap();
        harness.set_storage_of(CALLEE, b"init", b"1");
        let mut expected = REVERT_SELECTOR.to_vec();
        expected.extend_from_slice(b"ALKANES: revert: already initialized");
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data, expected);
    }

    #[test]
    fn a_contract_reads_the_height_block_and_transaction_of_its_context() {
        let contracts = Contracts::new("chain");
        let mut harness = contracts.harness(
            "chain",
            r#"(module
                (import "env" "__height" (func $height (param i32)))
                (import "env" "__request_block" (func $block_length (result i32)))
                (import "env" "__load_block" (func $block (param i32)))
                (import "env" "__request_transaction" (func $tx_length (result i32)))
                (import "env" "__load_transaction" (func $tx (param i32)))
                (memory (export "memory") 1)
                (func (export "__execute") (result i32)
                    (call $height (i32.const 20))
                    (i64.store (i32.const 124) (i64.load (i32.const 20)))
                    (call $block (i32.const 132))
                    (call $tx (i32.add (i32.const 132) (call $block_length)))
                    (i32.store (i32.const 100)
                        (i32.add (i32.const 28) (i32.add (call $block_length) (call $tx_length))))
                    (i32.const 104)))"#,
        );
        assert_eq!(
            harness.call_opcode(1, &[]).unwrap().data,
            0u64.to_le_bytes()
        );

        harness.context_mut().height = 840_000;
        harness.context_mut().block = b"block".to_vec();
        harness.context_mut().transaction = b"tx".to_vec();
        let mut expected = 840_000u64.to_le_bytes().to_vec();
        expected.extend_from_slice(b"blocktx");
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data, expected);
    }

    #[test]
    fn a_call_runs_the_callee_as_itself_and_keeps_what_it_stores() {
        let contracts = Contracts::new("call");
        let mut harness = contracts.harness("caller", &caller_wat("__call", CALLEE));
        harness
            .deploy_file(CALLEE, contracts.wasm("echo", ECHO_CONTEXT))
            .unwrap();
        let context = harness.call_opcode(1, &[]).unwrap().data;
        // myself, caller, vout, no transfers, then the opcode it was sent.
        assert_eq!(words(&context), vec![2, 9, 1, 1, 0, 0, 7]);

        harness
            .deploy_file(CALLEE, contracts.wasm("writer", WRITER))
            .unwrap();
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "pong");
        assert_eq!(harness.storage_value_of(CALLEE, b"hit"), Some(&b"yes"[..]));
        assert_eq!(harness.storage_value(b"hit"), None);
    }

    #[test]
    fn a_staticcall_returns_data_and_discards_what_the_callee_stores() {
        let contracts = Contracts::new("staticcall");
        let mut harness = contracts.harness("caller", &caller_wat("__staticcall", CALLEE));
        harness
            .deploy_file(CALLEE, contracts.wasm("writer", WRITER))
            .unwrap();
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "pong");
        assert_eq!(harness.storage_value_of(CALLEE, b"hit"), None);
    }

    #[test]
    fn a_delegatecall_runs_the_callee_as_the_caller() {
        let contracts = Contracts::new("delegatecall");
        let mut harness = contracts.harness("caller", &caller_wat("__delegatecall", CALLEE));
        harness
            .deploy_file(CALLEE, contracts.wasm("echo", ECHO_CONTEXT))
            .unwrap();
        let context = harness.call_opcode(1, &[]).unwrap().data;
        assert_eq!(words(&context), vec![1, 1, 2, 2, 0, 0, 7]);

        harness
            .deploy_file(CALLEE, contracts.wasm("writer", WRITER))
            .unwrap();
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "pong");
        assert_eq!(harness.storage_value(b"hit"), Some(&b"yes"[..]));
        assert_eq!(harness.storage_value_of(CALLEE, b"hit"), None);
    }

    #[test]
    fn a_reverted_or_missing_callee_is_reported_to_the_caller() {
        let contracts = Contracts::new("revert");
        let mut harness = contracts.harness("caller", &caller_wat("__call", CALLEE));
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "no");

        harness
            .deploy_file(CALLEE, contracts.wasm("trap", TRAP))
            .unwrap();
        harness.set_storage_of(CALLEE, b"kept", b"1");
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "no");
        assert_eq!(harness.storage_value_of(CALLEE, b"kept"), Some(&b"1"[..]));
    }

    #[test]
    fn a_failed_top_level_call_keeps_nothing_its_callees_stored() {
        let contracts = Contracts::new("rollback");
        // Calls the writer, then traps.
        let wat = caller_wat("__call", CALLEE).replace(
            "(call $copy (i32.const 220))",
            "(call $copy (i32.const 220)) unreachable",
        );
        let mut harness = contracts.harness("caller", &wat);
        harness
            .deploy_file(CALLEE, contracts.wasm("writer", WRITER))
            .unwrap();
        assert_revert(harness.call_opcode(1, &[]));
        assert_eq!(harness.storage_value_of(CALLEE, b"hit"), None);
    }

    #[test]
    fn the_block_header_precompile_answers_from_the_context_block() {
        let contracts = Contracts::new("header");
        let mut harness = contracts.harness(
            "caller",
            &caller_wat("__staticcall", BLOCK_HEADER_PRECOMPILE),
        );
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data_text(), "no");

        let block: Vec<u8> = (0..100).collect();
        harness.context_mut().block = block.clone();
        assert_eq!(harness.call_opcode(1, &[]).unwrap().data, block[..80]);
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
        assert_revert(harness.call_method("greet", &[]));

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
        assert_revert(harness.call_method("greet", &[]));
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
        harness.set_storage(b"counter", 1u128.to_le_bytes());
        harness.set_storage(b"not-a-u128", b"short");
        assert_eq!(harness.storage_u128(b"counter"), Some(1));
        assert_eq!(harness.storage_u128(b"missing"), None);
        assert_eq!(harness.storage_u128(b"not-a-u128"), None);
        assert_eq!(harness.call_method("tick", &[]).unwrap().data_text(), "");
        assert_eq!(harness.storage_value(b"key"), Some(&b"one"[..]));
        assert_eq!(harness.call_method("tick", &[]).unwrap().data_text(), "one");
        std::fs::remove_dir_all(root).ok();
    }
}
