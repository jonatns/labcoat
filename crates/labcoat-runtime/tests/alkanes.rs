//! Runs the real Alkanes indexer module through the runtime.
//!
//! The module is a release asset, not a file in this repository, so these
//! tests need its path in `LABCOAT_RUNTIME_ALKANES_WASM` and are ignored
//! otherwise:
//!
//! ```text
//! LABCOAT_RUNTIME_ALKANES_WASM=/path/to/alkanes.wasm \
//!     cargo test -p labcoat-runtime --test alkanes -- --ignored
//! ```
//!
//! Any `alkanes.wasm` built with the `regtest` feature works, including the
//! one published with a `cli-vX.Y.Z` release.

use bitcoin::ScriptBuf;
use labcoat_runtime::{IndexerModule, Runtime};

const MODULE_PATH: &str = "LABCOAT_RUNTIME_ALKANES_WASM";

fn runtime() -> Runtime {
    let path = std::env::var(MODULE_PATH)
        .unwrap_or_else(|_| panic!("set {MODULE_PATH} to the path of alkanes.wasm"));
    let mut runtime = Runtime::regtest(IndexerModule::from_file(path).unwrap()).unwrap();
    // A fixed clock, so two runtimes mine identical blocks.
    runtime.set_clock(|| 0);
    runtime
}

fn mine(runtime: &mut Runtime) -> u32 {
    // OP_TRUE: these blocks carry no Alkanes activity.
    runtime.mine(ScriptBuf::from_bytes(vec![0x51])).unwrap()
}

/// A protobuf request holding one `AlkaneId { block, tx: 0 }`, the shape of
/// `BytecodeRequest` and `AlkaneInventoryRequest`.
fn alkane_id_request(block: u8) -> Vec<u8> {
    assert!(block < 0x80, "one varint byte");
    vec![
        0x0a, 0x06, // id: AlkaneId
        0x0a, 0x02, 0x08, block, // block: uint128 { lo }
        0x12, 0x00, // tx: uint128 {}
    ]
}

#[test]
#[ignore = "needs alkanes.wasm; set LABCOAT_RUNTIME_ALKANES_WASM"]
fn indexes_blocks_and_serves_alkanes_views() {
    let mut runtime = runtime();
    assert!(
        !runtime.indexer().storage().is_empty(),
        "indexing the genesis block deploys the genesis alkanes"
    );

    for expected in 1..=3 {
        assert_eq!(mine(&mut runtime), expected);
    }
    assert_eq!(runtime.indexer().tip(), Some(3));

    // The next sequence number is a little-endian u128.
    assert_eq!(runtime.view("sequence", &[]).unwrap().len(), 16);

    // The genesis alkane 2:0 has WebAssembly bytecode; 9:0 does not exist.
    let bytecode = runtime.view("getbytecode", &alkane_id_request(2)).unwrap();
    assert!(bytecode.starts_with(b"\0asm"), "2:0 holds a wasm module");
    assert!(runtime
        .view("getbytecode", &alkane_id_request(9))
        .unwrap()
        .is_empty());

    // `BlockRequest { height: 1 }` returns the block this chain mined.
    let block = runtime.chain().block_bytes(1).unwrap();
    let response = runtime.view("getblock", &[0x08, 0x01]).unwrap();
    assert!(
        response.windows(block.len()).any(|window| window == block),
        "the indexer stored block 1 as mined"
    );
}

#[test]
#[ignore = "needs alkanes.wasm; set LABCOAT_RUNTIME_ALKANES_WASM"]
fn two_runtimes_reach_the_same_state() {
    let mut one = runtime();
    let mut two = runtime();
    for _ in 0..2 {
        mine(&mut one);
        mine(&mut two);
    }

    assert_eq!(one.chain().block_hash(2), two.chain().block_hash(2));
    assert_eq!(one.indexer().storage(), two.indexer().storage());
}

#[test]
#[ignore = "needs alkanes.wasm; set LABCOAT_RUNTIME_ALKANES_WASM"]
fn reverting_and_mining_again_reproduces_the_state() {
    let mut runtime = runtime();
    mine(&mut runtime);
    let snapshot = runtime.snapshot();
    let at_snapshot = runtime.indexer().storage().clone();

    mine(&mut runtime);
    mine(&mut runtime);
    let ahead = runtime.indexer().storage().clone();
    assert_ne!(at_snapshot, ahead);

    runtime.revert(snapshot).unwrap();
    assert_eq!(runtime.height(), 1);
    assert_eq!(runtime.indexer().storage(), &at_snapshot);

    mine(&mut runtime);
    mine(&mut runtime);
    assert_eq!(runtime.indexer().storage(), &ahead);
}
