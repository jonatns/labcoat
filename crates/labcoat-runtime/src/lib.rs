//! An in-process development runtime for Alkanes.
//!
//! This crate is the first building block of the direction proposed in
//! `docs/adr/0001-alkanes-development-runtime.md`. It is not used by the
//! `labcoat` CLI yet. It provides:
//!
//! - [`indexer`]: a host that runs a metashrew-compatible indexer module,
//!   such as `alkanes.wasm`, over state held in memory;
//! - [`chain`]: a simulated regtest chain that builds blocks;
//! - [`Runtime`]: the two together, so every mined block is indexed.
//!
//! The chain does not validate transactions yet; see [`chain`] for what is
//! missing.

pub mod chain;
pub mod indexer;

use bitcoin::{ScriptBuf, Transaction};

pub use chain::Chain;
pub use indexer::{Indexer, IndexerError, IndexerModule, IndexerSnapshot};

/// A point a [`Runtime`] can be reverted to.
#[derive(Clone)]
pub struct Snapshot {
    height: u32,
    indexer: IndexerSnapshot,
}

impl Snapshot {
    /// The chain height the snapshot was taken at.
    pub fn height(&self) -> u32 {
        self.height
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Indexer(#[from] IndexerError),
    #[error("the snapshot was taken at block {snapshot}, above the current block {current}")]
    SnapshotAhead { snapshot: u32, current: u32 },
}

/// A simulated chain whose blocks are indexed as they are mined.
pub struct Runtime {
    chain: Chain,
    indexer: Indexer,
}

impl Runtime {
    /// A regtest chain at its genesis block, with the genesis block indexed.
    pub fn regtest(module: IndexerModule) -> Result<Self, RuntimeError> {
        let chain = Chain::regtest();
        let mut indexer = Indexer::new(module);
        let genesis = chain.block_bytes(0).expect("a chain has a genesis block");
        indexer.index_block(0, &genesis)?;
        Ok(Self { chain, indexer })
    }

    /// The height of the newest block.
    pub fn height(&self) -> u32 {
        self.chain.height()
    }

    pub fn chain(&self) -> &Chain {
        &self.chain
    }

    pub fn indexer(&self) -> &Indexer {
        &self.indexer
    }

    /// Mine a block holding `transactions` and index it. Returns the new
    /// height. If the indexer rejects the block, the block is dropped and
    /// the runtime stays where it was.
    pub fn mine(
        &mut self,
        transactions: Vec<Transaction>,
        reward: ScriptBuf,
    ) -> Result<u32, RuntimeError> {
        let height = self.chain.mine(transactions, reward);
        let block = self
            .chain
            .block_bytes(height)
            .expect("the block just mined");
        if let Err(error) = self.indexer.index_block(height, &block) {
            self.chain.truncate(height - 1);
            return Err(error.into());
        }
        Ok(height)
    }

    /// Call an indexer view against the state as of the newest block.
    pub fn view(&self, name: &str, input: &[u8]) -> Result<Vec<u8>, RuntimeError> {
        Ok(self.indexer.view(name, input)?)
    }

    /// Record the current chain and indexer state.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            height: self.chain.height(),
            indexer: self.indexer.snapshot(),
        }
    }

    /// Return to a recorded state, dropping every block mined since.
    ///
    /// A snapshot is only valid on the chain it was taken from, at or below
    /// the current height.
    pub fn revert(&mut self, snapshot: Snapshot) -> Result<(), RuntimeError> {
        let current = self.chain.height();
        if snapshot.height > current {
            return Err(RuntimeError::SnapshotAhead {
                snapshot: snapshot.height,
                current,
            });
        }
        self.chain.truncate(snapshot.height);
        self.indexer.restore(snapshot.indexer);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the height of each block under `tip`; refuses block 3.
    const RECORDER: &str = r#"
        (module
          (import "env" "__load_input" (func $load_input (param i32)))
          (import "env" "__get_len" (func $get_len (param i32) (result i32)))
          (import "env" "__get" (func $get (param i32 i32)))
          (import "env" "__flush" (func $flush (param i32)))
          (memory (export "memory") 4)
          (data (i32.const 2044) "\0b\00\00\00")
          (data (i32.const 2048) "\0a\03tip\0a\04\00\00\00\00")
          (data (i32.const 2996) "\03\00\00\00tip")
          (func (export "_start")
            (call $load_input (i32.const 8192))
            (if (i32.eq (i32.load (i32.const 8192)) (i32.const 3)) (then unreachable))
            (i32.store (i32.const 2055) (i32.load (i32.const 8192)))
            (call $flush (i32.const 2048)))
          (func (export "tip") (result i32)
            (i32.store (i32.const 3996) (call $get_len (i32.const 3000)))
            (call $get (i32.const 3000) (i32.const 4000))
            (i32.const 4000)))
    "#;

    fn runtime() -> Runtime {
        let wasm = wat::parse_str(RECORDER).unwrap();
        Runtime::regtest(IndexerModule::from_bytes(&wasm).unwrap()).unwrap()
    }

    fn reward() -> ScriptBuf {
        ScriptBuf::from_bytes(vec![0x51])
    }

    fn tip(runtime: &Runtime) -> u32 {
        u32::from_le_bytes(runtime.view("tip", &[]).unwrap().try_into().unwrap())
    }

    #[test]
    fn indexes_genesis_and_every_mined_block() {
        let mut runtime = runtime();
        assert_eq!((runtime.height(), tip(&runtime)), (0, 0));

        assert_eq!(runtime.mine(Vec::new(), reward()).unwrap(), 1);
        assert_eq!(runtime.mine(Vec::new(), reward()).unwrap(), 2);
        assert_eq!((runtime.height(), tip(&runtime)), (2, 2));
        assert_eq!(runtime.indexer().tip(), Some(2));
    }

    #[test]
    fn drops_a_block_the_indexer_rejects() {
        let mut runtime = runtime();
        runtime.mine(Vec::new(), reward()).unwrap();
        runtime.mine(Vec::new(), reward()).unwrap();

        let error = runtime.mine(Vec::new(), reward()).unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::Indexer(IndexerError::Trap { .. })
        ));
        assert_eq!((runtime.height(), tip(&runtime)), (2, 2));
        assert!(runtime.chain().block(3).is_none());
    }

    #[test]
    fn reverts_to_a_snapshot() {
        let mut runtime = runtime();
        runtime.mine(Vec::new(), reward()).unwrap();
        let snapshot = runtime.snapshot();
        let hash = runtime.chain().block_hash(1);
        runtime.mine(Vec::new(), reward()).unwrap();

        runtime.revert(snapshot.clone()).unwrap();
        assert_eq!((runtime.height(), tip(&runtime)), (1, 1));
        assert_eq!(runtime.chain().block_hash(1), hash);
        assert_eq!(runtime.mine(Vec::new(), reward()).unwrap(), 2);

        // A snapshot from above the current height does not apply.
        let later = runtime.snapshot();
        runtime.revert(snapshot).unwrap();
        assert!(matches!(
            runtime.revert(later),
            Err(RuntimeError::SnapshotAhead {
                snapshot: 2,
                current: 1
            })
        ));
    }
}
