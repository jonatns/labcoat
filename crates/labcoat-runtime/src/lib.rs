//! Labcoat Network's development runtime: a Bitcoin regtest chain and the
//! Alkanes indexer, run in one process.
//!
//! See `docs/adr/0001-alkanes-development-runtime.md` for why it exists.
//!
//! - [`chain`] builds and stores blocks.
//! - [`ledger`] tracks unspent outputs and the mempool.
//! - [`validate`] decides which transactions the mempool accepts.
//! - [`indexer`] runs a metashrew-compatible module such as `alkanes.wasm`.
//! - [`store`] saves blocks and indexer writes so a restart is instant.
//! - [`esplora`] answers the Esplora queries wallets make.
//! - [`node`] and [`server`] serve all of it over Bitcoin Core-style
//!   JSON-RPC, which is how the `labcoat` CLI talks to Labcoat Network;
//!   [`daemon`] runs that server as the `labcoat node` process.
//!
//! [`Runtime`] ties the parts together: every block it mines is checked,
//! indexed, applied, and saved, or not mined at all.

pub mod chain;
pub mod daemon;
pub mod esplora;
pub mod indexer;
pub mod ledger;
pub mod node;
pub mod server;
pub mod store;
pub mod validate;

use std::path::Path;

use bitcoin::consensus::serialize;
use bitcoin::{ScriptBuf, Transaction, Txid};

pub use chain::Chain;
pub use indexer::{Indexer, IndexerError, IndexerModule, IndexerSnapshot};
pub use ledger::{Ledger, Mempool};
pub use validate::{Accepted, Rejection};

use ledger::Entry;
use store::{BlockLog, Opened, StoreError};

/// A point a [`Runtime`] can be reverted to.
#[derive(Clone)]
pub struct Snapshot {
    height: u32,
    ledger: Ledger,
    mempool: Mempool,
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
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Chain(#[from] chain::ChainError),
    #[error(transparent)]
    Rejected(#[from] Rejection),
    #[error("the snapshot was taken at block {snapshot}, above the current block {current}")]
    SnapshotAhead { snapshot: u32, current: u32 },
    #[error("the saved chain does not start at the regtest genesis block; run `labcoat reset`")]
    ForeignGenesis,
}

/// Seconds since the Unix epoch, for block times.
pub type Clock = fn() -> u32;

fn system_clock() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as u32)
}

/// A regtest chain whose blocks are indexed as they are mined.
pub struct Runtime {
    chain: Chain,
    ledger: Ledger,
    mempool: Mempool,
    indexer: Indexer,
    log: Option<BlockLog>,
    clock: Clock,
}

impl Runtime {
    /// A chain at its genesis block, held in memory only.
    pub fn regtest(module: IndexerModule) -> Result<Self, RuntimeError> {
        let mut runtime = Self::empty(module);
        runtime.index_genesis()?;
        Ok(runtime)
    }

    /// The chain saved at `path`, or a new one saved there.
    ///
    /// A chain saved with the same indexer module reloads without running
    /// the module. One saved with a different module is indexed again from
    /// its blocks, and the file is rewritten.
    pub fn open(module: IndexerModule, path: &Path) -> Result<Self, RuntimeError> {
        let hash = module.hash();
        let mut runtime = Self::empty(module);
        match BlockLog::open(path, &hash)? {
            Opened::Matching(log, records) => {
                let mut records = records.into_iter();
                let genesis = records.next().ok_or(RuntimeError::ForeignGenesis)?;
                if genesis.block.block_hash() != runtime.chain.tip().block_hash() {
                    return Err(RuntimeError::ForeignGenesis);
                }
                runtime.indexer.apply_writes(0, &genesis.writes)?;
                runtime.ledger.apply_block(0, &genesis.block);
                for record in records {
                    let height = runtime.chain.push(record.block)?;
                    let block = runtime.chain.tip();
                    runtime.indexer.apply_writes(height, &record.writes)?;
                    runtime.ledger.apply_block(height, block);
                }
                runtime.log = Some(log);
            }
            Opened::DifferentModule(blocks) => {
                tracing::info!(
                    "the indexer module changed; indexing {} saved blocks again",
                    blocks.len()
                );
                let rebuilt = path.with_extension("rebuild");
                let mut log = BlockLog::create(&rebuilt, &hash)?;
                let mut blocks = blocks.into_iter();
                let genesis = blocks.next().ok_or(RuntimeError::ForeignGenesis)?;
                if genesis.block_hash() != runtime.chain.tip().block_hash() {
                    return Err(RuntimeError::ForeignGenesis);
                }
                let writes = runtime.indexer.index_block(0, &serialize(&genesis))?;
                runtime.ledger.apply_block(0, &genesis);
                log.append(0, &genesis, &writes)?;
                for block in blocks {
                    let bytes = serialize(&block);
                    let height = runtime.chain.push(block)?;
                    let writes = runtime.indexer.index_block(height, &bytes)?;
                    runtime.ledger.apply_block(height, runtime.chain.tip());
                    log.append(height, runtime.chain.tip(), &writes)?;
                }
                drop(log);
                std::fs::rename(&rebuilt, path).map_err(|source| StoreError::Io {
                    path: path.to_owned(),
                    source,
                })?;
                runtime.log = Some(match BlockLog::open(path, &hash)? {
                    Opened::Matching(log, _) => log,
                    _ => unreachable!("the file was just written with this module"),
                });
            }
            Opened::Missing => {
                runtime.log = Some(BlockLog::create(path, &hash)?);
                runtime.index_genesis()?;
            }
        }
        Ok(runtime)
    }

    fn empty(module: IndexerModule) -> Self {
        Self {
            chain: Chain::regtest(),
            ledger: Ledger::default(),
            mempool: Mempool::default(),
            indexer: Indexer::new(module),
            log: None,
            clock: system_clock,
        }
    }

    fn index_genesis(&mut self) -> Result<(), RuntimeError> {
        let genesis = self
            .chain
            .block(0)
            .expect("a chain has a genesis block")
            .clone();
        let writes = self.indexer.index_block(0, &serialize(&genesis))?;
        self.ledger.apply_block(0, &genesis);
        if let Some(log) = &mut self.log {
            log.append(0, &genesis, &writes)?;
        }
        Ok(())
    }

    /// Use `clock` for the times of blocks mined from now on. Block times
    /// still increase by at least a second per block.
    pub fn set_clock(&mut self, clock: Clock) {
        self.clock = clock;
    }

    /// The height of the newest block.
    pub fn height(&self) -> u32 {
        self.chain.height()
    }

    pub fn chain(&self) -> &Chain {
        &self.chain
    }

    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    pub fn mempool(&self) -> &Mempool {
        &self.mempool
    }

    pub fn indexer(&self) -> &Indexer {
        &self.indexer
    }

    fn context(&self) -> validate::Context<'_> {
        validate::Context {
            ledger: &self.ledger,
            mempool: &self.mempool,
            next_height: self.chain.height() + 1,
            tip_time: self.chain.tip().header.time,
        }
    }

    /// Check `tx` as [`submit`](Runtime::submit) would, without adding it.
    pub fn test(&self, tx: &Transaction, max_fee_rate: Option<u64>) -> Result<Accepted, Rejection> {
        validate::check(tx, &self.context(), max_fee_rate)
    }

    /// Add `tx` to the mempool if it passes every check.
    pub fn submit(
        &mut self,
        tx: Transaction,
        max_fee_rate: Option<u64>,
    ) -> Result<Txid, Rejection> {
        let accepted = self.test(&tx, max_fee_rate)?;
        let txid = tx.compute_txid();
        self.mempool.add(Entry {
            tx,
            txid,
            fee: accepted.fee,
            weight: accepted.weight,
        });
        Ok(txid)
    }

    /// Mine a block holding every mempool transaction and paying `reward`,
    /// then index and save it. Returns the new height. If the indexer
    /// rejects the block, it is dropped and the mempool kept.
    pub fn mine(&mut self, reward: ScriptBuf) -> Result<u32, RuntimeError> {
        let entries = self.mempool.take();
        let fees = entries.iter().map(|entry| entry.fee).sum();
        let transactions = entries.iter().map(|entry| entry.tx.clone()).collect();
        let height = self.chain.mine(transactions, reward, fees, (self.clock)());
        let bytes = self
            .chain
            .block_bytes(height)
            .expect("the block just mined");

        let writes = match self.indexer.index_block(height, &bytes) {
            Ok(writes) => writes,
            Err(error) => {
                self.chain.truncate(height - 1);
                for entry in entries {
                    self.mempool.add(entry);
                }
                return Err(error.into());
            }
        };
        let block = self.chain.tip();
        self.ledger.apply_block(height, block);
        if let Some(log) = &mut self.log {
            log.append(height, block, &writes)?;
        }
        Ok(height)
    }

    /// Call an indexer view against the state as of the newest block.
    pub fn view(&self, name: &str, input: &[u8]) -> Result<Vec<u8>, RuntimeError> {
        Ok(self.indexer.view(name, input)?)
    }

    /// Record the current chain, mempool, and indexer state.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            height: self.chain.height(),
            ledger: self.ledger.clone(),
            mempool: self.mempool.clone(),
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
        if let Some(log) = &mut self.log {
            log.truncate(snapshot.height)?;
        }
        self.chain.truncate(snapshot.height);
        self.ledger = snapshot.ledger;
        self.mempool = snapshot.mempool;
        self.indexer.restore(snapshot.indexer);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
