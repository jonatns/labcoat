//! A host for metashrew-compatible indexer modules such as `alkanes.wasm`.
//!
//! An indexer module is a WebAssembly program that is handed one block at a
//! time and writes what it learns to a key-value store. It reaches the host
//! through a handful of imports in the `env` namespace:
//!
//! | Import | Meaning |
//! |---|---|
//! | `__host_len() -> i32` | Length of the input: four bytes of height plus the payload |
//! | `__load_input(ptr)` | Copy the little-endian height and the payload to `ptr` |
//! | `__get_len(key) -> i32` | Length of the value stored under `key`, zero if absent |
//! | `__get(key, value)` | Copy the value stored under `key` to `value` |
//! | `__flush(pairs)` | Write a list of key-value pairs |
//! | `__log(message)` | Emit a debug line |
//!
//! Every pointer except the one given to `__load_input` and the destination
//! of `__get` addresses a length-prefixed buffer: the bytes start at the
//! pointer and their little-endian `u32` length sits in the four bytes before
//! it.
//!
//! Indexing a block calls the module's `_start` export with the serialized
//! block as the payload. A view is any other export of type `() -> i32`: it
//! reads its request from the same input functions and returns a pointer to a
//! length-prefixed response. Writes made during a view are discarded.
//!
//! The module never sees how the host stores data, so this host keeps a plain
//! ordered map of the latest value per key. It does not keep per-height
//! history: a view always reads the state as of the last indexed block.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use wasmtime::{Caller, Config, Engine, Linker, Memory, Module, Store};

/// The state an indexer module has written: the latest value for each key.
pub type Storage = BTreeMap<Vec<u8>, Vec<u8>>;

/// The export an indexer module runs to index one block.
const INDEX_EXPORT: &str = "_start";

#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error("the indexer module could not be loaded: {0}")]
    Module(String),
    #[error("the indexer module has no `{0}` export of the expected type")]
    MissingExport(String),
    #[error("the indexer module stopped while running `{export}`: {message}")]
    Trap { export: String, message: String },
    #[error("the indexer module returned a response outside its memory")]
    InvalidResponse,
    #[error("block {given} cannot be indexed after block {tip}: the next block is {expected}")]
    OutOfOrder { given: u32, tip: u32, expected: u32 },
    #[error("block {given} cannot be indexed first: an empty indexer starts at block 0")]
    NotGenesis { given: u32 },
    #[error("nothing has been indexed yet")]
    Empty,
}

/// A compiled indexer module. Compiling is the slow part, so compile once and
/// share the result between every [`Indexer`] that runs the same module.
#[derive(Clone)]
pub struct IndexerModule {
    engine: Engine,
    module: Module,
}

impl IndexerModule {
    /// Compile a module from WebAssembly bytes.
    pub fn from_bytes(wasm: &[u8]) -> Result<Self, IndexerError> {
        let engine = engine()?;
        let module = Module::new(&engine, wasm).map_err(module_error)?;
        Ok(Self { engine, module })
    }

    /// Compile a module from a `.wasm` file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, IndexerError> {
        let engine = engine()?;
        let module = Module::from_file(&engine, path.as_ref()).map_err(module_error)?;
        Ok(Self { engine, module })
    }

    /// The names of the module's exported functions.
    pub fn exports(&self) -> Vec<String> {
        self.module
            .exports()
            .filter(|export| export.ty().func().is_some())
            .map(|export| export.name().to_owned())
            .collect()
    }
}

fn engine() -> Result<Engine, IndexerError> {
    let mut config = Config::new();
    // Indexers must compute the same state on every machine.
    config.cranelift_nan_canonicalization(true);
    Engine::new(&config).map_err(module_error)
}

fn module_error(error: wasmtime::Error) -> IndexerError {
    IndexerError::Module(format!("{error:#}"))
}

/// A point an [`Indexer`] can be returned to. Taking one is cheap: it shares
/// the stored state until the indexer next writes.
#[derive(Clone)]
pub struct IndexerSnapshot {
    storage: Arc<Storage>,
    tip: Option<u32>,
}

/// One indexer module and the state it has built by indexing blocks in order.
pub struct Indexer {
    module: IndexerModule,
    storage: Arc<Storage>,
    tip: Option<u32>,
}

impl Indexer {
    /// An indexer that has seen no blocks.
    pub fn new(module: IndexerModule) -> Self {
        Self {
            module,
            storage: Arc::new(Storage::new()),
            tip: None,
        }
    }

    /// The height of the last indexed block.
    pub fn tip(&self) -> Option<u32> {
        self.tip
    }

    /// The state the module has written so far.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Index the block at `height`, given in consensus serialization.
    ///
    /// Blocks are indexed in order, starting at 0. The block's writes are
    /// applied only if the module finishes; a module that stops part way
    /// leaves the state as it was.
    pub fn index_block(&mut self, height: u32, block: &[u8]) -> Result<(), IndexerError> {
        match self.tip {
            None if height != 0 => return Err(IndexerError::NotGenesis { given: height }),
            Some(tip) if height != tip + 1 => {
                return Err(IndexerError::OutOfOrder {
                    given: height,
                    tip,
                    expected: tip + 1,
                })
            }
            _ => {}
        }

        let state = HostState::new(height, block, self.storage.clone(), Mode::Index);
        let mut store = Store::new(&self.module.engine, state);
        let instance = self.instantiate(&mut store)?;
        let start = instance
            .get_typed_func::<(), ()>(&mut store, INDEX_EXPORT)
            .map_err(|_| IndexerError::MissingExport(INDEX_EXPORT.to_owned()))?;
        start
            .call(&mut store, ())
            .map_err(|error| trap(INDEX_EXPORT, error))?;

        let written = store.into_data().written;
        if !written.is_empty() {
            Arc::make_mut(&mut self.storage).extend(written);
        }
        self.tip = Some(height);
        Ok(())
    }

    /// Call the view `name` with `input` against the state as of the tip.
    pub fn view(&self, name: &str, input: &[u8]) -> Result<Vec<u8>, IndexerError> {
        let height = self.tip.ok_or(IndexerError::Empty)?;
        let state = HostState::new(height, input, self.storage.clone(), Mode::View);
        let mut store = Store::new(&self.module.engine, state);
        let instance = self.instantiate(&mut store)?;
        let view = instance
            .get_typed_func::<(), i32>(&mut store, name)
            .map_err(|_| IndexerError::MissingExport(name.to_owned()))?;
        let pointer = view
            .call(&mut store, ())
            .map_err(|error| trap(name, error))?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or(IndexerError::InvalidResponse)?;
        read_buffer(memory.data(&store), pointer).ok_or(IndexerError::InvalidResponse)
    }

    /// Record the current state so it can be returned to with [`restore`].
    ///
    /// [`restore`]: Indexer::restore
    pub fn snapshot(&self) -> IndexerSnapshot {
        IndexerSnapshot {
            storage: self.storage.clone(),
            tip: self.tip,
        }
    }

    /// Return to a state recorded by [`snapshot`].
    ///
    /// [`snapshot`]: Indexer::snapshot
    pub fn restore(&mut self, snapshot: IndexerSnapshot) {
        self.storage = snapshot.storage;
        self.tip = snapshot.tip;
    }

    fn instantiate(
        &self,
        store: &mut Store<HostState>,
    ) -> Result<wasmtime::Instance, IndexerError> {
        let mut linker = Linker::new(&self.module.engine);
        add_host_functions(&mut linker).map_err(module_error)?;
        linker
            .instantiate(store, &self.module.module)
            .map_err(module_error)
    }
}

fn trap(export: &str, error: wasmtime::Error) -> IndexerError {
    IndexerError::Trap {
        export: export.to_owned(),
        message: format!("{error:#}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Index,
    View,
}

struct HostState {
    /// The little-endian height followed by the block or the view request.
    input: Vec<u8>,
    storage: Arc<Storage>,
    /// Writes made by this call, read back before `storage`.
    written: Storage,
    mode: Mode,
}

impl HostState {
    fn new(height: u32, payload: &[u8], storage: Arc<Storage>, mode: Mode) -> Self {
        let mut input = Vec::with_capacity(payload.len() + 4);
        input.extend_from_slice(&height.to_le_bytes());
        input.extend_from_slice(payload);
        Self {
            input,
            storage,
            written: Storage::new(),
            mode,
        }
    }

    fn get(&self, key: &[u8]) -> Option<&Vec<u8>> {
        self.written.get(key).or_else(|| self.storage.get(key))
    }
}

fn add_host_functions(linker: &mut Linker<HostState>) -> wasmtime::Result<()> {
    linker.func_wrap(
        "env",
        "__host_len",
        |caller: Caller<'_, HostState>| -> wasmtime::Result<i32> {
            i32::try_from(caller.data().input.len())
                .map_err(|_| wasmtime::format_err!("input is too large for the module"))
        },
    )?;

    linker.func_wrap(
        "env",
        "__load_input",
        |mut caller: Caller<'_, HostState>, pointer: i32| -> wasmtime::Result<()> {
            let memory = memory(&mut caller)?;
            let (data, state) = memory.data_and_store_mut(&mut caller);
            write(data, pointer, &state.input)
        },
    )?;

    linker.func_wrap(
        "env",
        "__get_len",
        |mut caller: Caller<'_, HostState>, key: i32| -> wasmtime::Result<i32> {
            let memory = memory(&mut caller)?;
            let (data, state) = memory.data_and_store_mut(&mut caller);
            let key = read_buffer(data, key)
                .ok_or_else(|| wasmtime::format_err!("__get_len key is outside memory"))?;
            let length = state.get(&key).map_or(0, Vec::len);
            i32::try_from(length)
                .map_err(|_| wasmtime::format_err!("stored value is too large for the module"))
        },
    )?;

    linker.func_wrap(
        "env",
        "__get",
        |mut caller: Caller<'_, HostState>, key: i32, value: i32| -> wasmtime::Result<()> {
            let memory = memory(&mut caller)?;
            let (data, state) = memory.data_and_store_mut(&mut caller);
            let key = read_buffer(data, key)
                .ok_or_else(|| wasmtime::format_err!("__get key is outside memory"))?;
            match state.get(&key) {
                Some(stored) => write(data, value, stored),
                None => Ok(()),
            }
        },
    )?;

    linker.func_wrap(
        "env",
        "__flush",
        |mut caller: Caller<'_, HostState>, pairs: i32| -> wasmtime::Result<()> {
            let memory = memory(&mut caller)?;
            let (data, state) = memory.data_and_store_mut(&mut caller);
            if state.mode == Mode::View {
                return Ok(());
            }
            let encoded = read_buffer(data, pairs)
                .ok_or_else(|| wasmtime::format_err!("__flush data is outside memory"))?;
            let list = decode_flush(&encoded)
                .ok_or_else(|| wasmtime::format_err!("__flush data is not a key-value list"))?;
            if list.len() % 2 != 0 {
                wasmtime::bail!("__flush data ends with a key that has no value");
            }
            let mut list = list.into_iter();
            while let (Some(key), Some(value)) = (list.next(), list.next()) {
                state.written.insert(key, value);
            }
            Ok(())
        },
    )?;

    linker.func_wrap(
        "env",
        "__log",
        |mut caller: Caller<'_, HostState>, message: i32| -> wasmtime::Result<()> {
            let memory = memory(&mut caller)?;
            if let Some(bytes) = read_buffer(memory.data(&caller), message) {
                tracing::debug!(target: "labcoat_runtime::indexer", "{}", String::from_utf8_lossy(&bytes));
            }
            Ok(())
        },
    )?;

    Ok(())
}

fn memory(caller: &mut Caller<'_, HostState>) -> wasmtime::Result<Memory> {
    caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| wasmtime::format_err!("the module exports no memory"))
}

/// Read the length-prefixed buffer whose bytes start at `pointer`.
fn read_buffer(memory: &[u8], pointer: i32) -> Option<Vec<u8>> {
    let start = usize::try_from(pointer).ok()?;
    let prefix = memory.get(start.checked_sub(4)?..start)?;
    let length = u32::from_le_bytes(prefix.try_into().ok()?) as usize;
    memory
        .get(start..start.checked_add(length)?)
        .map(<[u8]>::to_vec)
}

fn write(memory: &mut [u8], pointer: i32, bytes: &[u8]) -> wasmtime::Result<()> {
    let start = usize::try_from(pointer)
        .map_err(|_| wasmtime::format_err!("the module passed a negative pointer"))?;
    let end = start
        .checked_add(bytes.len())
        .filter(|end| *end <= memory.len())
        .ok_or_else(|| wasmtime::format_err!("the module passed a buffer outside its memory"))?;
    memory[start..end].copy_from_slice(bytes);
    Ok(())
}

/// Decode a protobuf `KeyValueFlush { repeated bytes list = 1; }` message:
/// alternating keys and values.
fn decode_flush(mut encoded: &[u8]) -> Option<Vec<Vec<u8>>> {
    const LIST_FIELD: u64 = 1;
    const LENGTH_DELIMITED: u64 = 2;

    let mut list = Vec::new();
    while !encoded.is_empty() {
        let tag = read_varint(&mut encoded)?;
        if tag >> 3 != LIST_FIELD || tag & 7 != LENGTH_DELIMITED {
            return None;
        }
        let length = usize::try_from(read_varint(&mut encoded)?).ok()?;
        if length > encoded.len() {
            return None;
        }
        let (item, rest) = encoded.split_at(length);
        list.push(item.to_vec());
        encoded = rest;
    }
    Some(list)
}

fn read_varint(bytes: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (byte, rest) = bytes.split_first()?;
        *bytes = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module that records the height and input length of each block under
    /// the keys `tip` and `len`, and serves three views.
    const RECORDER: &str = r#"
        (module
          (import "env" "__host_len" (func $host_len (result i32)))
          (import "env" "__load_input" (func $load_input (param i32)))
          (import "env" "__get_len" (func $get_len (param i32) (result i32)))
          (import "env" "__get" (func $get (param i32 i32)))
          (import "env" "__flush" (func $flush (param i32)))
          (memory (export "memory") 1)

          ;; Flush message at 2048: "tip" -> height, "len" -> input length.
          (data (i32.const 2044) "\16\00\00\00")
          (data (i32.const 2048) "\0a\03tip\0a\04\00\00\00\00\0a\03len\0a\04\00\00\00\00")
          ;; Keys at 3000, 3100, and 3200.
          (data (i32.const 2996) "\03\00\00\00tip")
          (data (i32.const 3096) "\03\00\00\00len")
          (data (i32.const 3196) "\04\00\00\00nope")

          (func (export "_start")
            (call $load_input (i32.const 1024))
            (i32.store (i32.const 2055) (i32.load (i32.const 1024)))
            (i32.store (i32.const 2066) (call $host_len))
            (call $flush (i32.const 2048)))

          (func $read (param $key i32) (result i32)
            (i32.store (i32.const 3996) (call $get_len (local.get $key)))
            (call $get (local.get $key) (i32.const 4000))
            (i32.const 4000))

          (func (export "tip") (result i32) (call $read (i32.const 3000)))
          (func (export "len") (result i32) (call $read (i32.const 3100)))
          (func (export "nope") (result i32) (call $read (i32.const 3200)))

          ;; Returns the input exactly as the host presents it.
          (func (export "echo") (result i32)
            (i32.store (i32.const 4996) (call $host_len))
            (call $load_input (i32.const 5000))
            (i32.const 5000))

          ;; A view that tries to write.
          (func (export "scribble") (result i32)
            (call $load_input (i32.const 1024))
            (i32.store (i32.const 2055) (i32.const 999))
            (call $flush (i32.const 2048))
            (call $read (i32.const 3000)))

          (func (export "boom") (result i32) unreachable))
    "#;

    /// A module whose `_start` writes and then stops.
    const FAILS_AFTER_WRITING: &str = r#"
        (module
          (import "env" "__flush" (func $flush (param i32)))
          (memory (export "memory") 1)
          (data (i32.const 2044) "\07\00\00\00")
          (data (i32.const 2048) "\0a\01k\0a\02vv")
          (func (export "_start")
            (call $flush (i32.const 2048))
            unreachable))
    "#;

    fn indexer(source: &str) -> Indexer {
        let wasm = wat::parse_str(source).unwrap();
        Indexer::new(IndexerModule::from_bytes(&wasm).unwrap())
    }

    #[test]
    fn indexes_blocks_in_order_and_serves_views() {
        let mut indexer = indexer(RECORDER);
        assert_eq!(indexer.tip(), None);
        assert!(matches!(indexer.view("tip", &[]), Err(IndexerError::Empty)));

        indexer.index_block(0, b"genesis").unwrap();
        indexer.index_block(1, b"a longer block").unwrap();

        assert_eq!(indexer.tip(), Some(1));
        assert_eq!(indexer.view("tip", &[]).unwrap(), 1u32.to_le_bytes());
        assert_eq!(indexer.view("len", &[]).unwrap(), 18u32.to_le_bytes());
        assert_eq!(indexer.view("nope", &[]).unwrap(), Vec::<u8>::new());
        assert_eq!(indexer.storage().len(), 2);
    }

    #[test]
    fn presents_the_height_before_the_input() {
        let mut indexer = indexer(RECORDER);
        indexer.index_block(0, b"").unwrap();
        indexer.index_block(1, b"").unwrap();

        let mut expected = 1u32.to_le_bytes().to_vec();
        expected.extend_from_slice(b"request");
        assert_eq!(indexer.view("echo", b"request").unwrap(), expected);
    }

    #[test]
    fn refuses_blocks_out_of_order() {
        let mut indexer = indexer(RECORDER);
        assert!(matches!(
            indexer.index_block(3, b""),
            Err(IndexerError::NotGenesis { given: 3 })
        ));
        indexer.index_block(0, b"").unwrap();
        assert!(matches!(
            indexer.index_block(2, b""),
            Err(IndexerError::OutOfOrder {
                given: 2,
                tip: 0,
                expected: 1
            })
        ));
        assert!(matches!(
            indexer.index_block(0, b""),
            Err(IndexerError::OutOfOrder { .. })
        ));
    }

    #[test]
    fn a_view_cannot_change_state() {
        let mut indexer = indexer(RECORDER);
        indexer.index_block(0, b"").unwrap();

        // Inside the view the write is not visible either: views read the
        // indexed state only.
        assert_eq!(indexer.view("scribble", &[]).unwrap(), 0u32.to_le_bytes());
        assert_eq!(indexer.view("tip", &[]).unwrap(), 0u32.to_le_bytes());
    }

    #[test]
    fn a_block_that_fails_leaves_no_writes() {
        let mut indexer = indexer(FAILS_AFTER_WRITING);
        let error = indexer.index_block(0, b"").unwrap_err();

        assert!(matches!(error, IndexerError::Trap { ref export, .. } if export == "_start"));
        assert_eq!(indexer.tip(), None);
        assert!(indexer.storage().is_empty());
    }

    #[test]
    fn reports_missing_and_failing_views() {
        let mut indexer = indexer(RECORDER);
        indexer.index_block(0, b"").unwrap();

        assert!(matches!(
            indexer.view("absent", &[]),
            Err(IndexerError::MissingExport(name)) if name == "absent"
        ));
        assert!(matches!(
            indexer.view("boom", &[]),
            Err(IndexerError::Trap { ref export, .. }) if export == "boom"
        ));
    }

    #[test]
    fn restores_a_snapshot() {
        let mut indexer = indexer(RECORDER);
        indexer.index_block(0, b"").unwrap();
        let snapshot = indexer.snapshot();

        indexer.index_block(1, b"").unwrap();
        indexer.index_block(2, b"").unwrap();
        assert_eq!(indexer.view("tip", &[]).unwrap(), 2u32.to_le_bytes());

        indexer.restore(snapshot);
        assert_eq!(indexer.tip(), Some(0));
        assert_eq!(indexer.view("tip", &[]).unwrap(), 0u32.to_le_bytes());
        indexer.index_block(1, b"").unwrap();
    }

    #[test]
    fn lists_exported_functions() {
        let wasm = wat::parse_str(RECORDER).unwrap();
        let exports = IndexerModule::from_bytes(&wasm).unwrap().exports();
        assert!(exports.contains(&"_start".to_owned()));
        assert!(exports.contains(&"echo".to_owned()));
        assert!(!exports.contains(&"memory".to_owned()));
    }

    #[test]
    fn decodes_a_flush_message() {
        assert_eq!(
            decode_flush(b"\x0a\x01k\x0a\x02vv"),
            Some(vec![b"k".to_vec(), b"vv".to_vec()])
        );
        assert_eq!(decode_flush(b""), Some(Vec::new()));
        // A length that runs past the end, and a field that is not the list.
        assert_eq!(decode_flush(b"\x0a\x05k"), None);
        assert_eq!(decode_flush(b"\x10\x01"), None);
    }
}
