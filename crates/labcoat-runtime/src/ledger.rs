//! What the chain's blocks add up to: the unspent outputs, where every
//! transaction is, and which transaction spent each output. Also the mempool,
//! the transactions accepted but not yet mined.

use std::collections::{BTreeSet, HashMap};

use bitcoin::hashes::{sha256, Hash};
use bitcoin::{Amount, Block, OutPoint, Script, Transaction, TxOut, Txid, Weight};

/// An unspent output and where it was created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coin {
    pub output: TxOut,
    /// The height of the block that created it. For an output of a mempool
    /// transaction, the height of the next block.
    pub height: u32,
    pub coinbase: bool,
}

/// Where a confirmed transaction is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    pub height: u32,
    /// The transaction's index in its block; 0 is the coinbase.
    pub position: u32,
}

/// The transaction input that spent an output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spender {
    pub txid: Txid,
    pub input: u32,
}

/// The SHA-256 of an output script: how Esplora indexes outputs by script.
pub type ScriptHash = [u8; 32];

pub fn script_hash(script: &Script) -> ScriptHash {
    sha256::Hash::hash(script.as_bytes()).to_byte_array()
}

/// The state of the confirmed chain.
#[derive(Clone, Default)]
pub struct Ledger {
    coins: HashMap<OutPoint, Coin>,
    by_script: HashMap<ScriptHash, BTreeSet<OutPoint>>,
    locations: HashMap<Txid, Location>,
    spenders: HashMap<OutPoint, Spender>,
}

impl Ledger {
    /// Apply the block at `height`. The genesis block's output cannot be
    /// spent, as in Bitcoin, so it adds no coins.
    pub fn apply_block(&mut self, height: u32, block: &Block) {
        for (position, tx) in block.txdata.iter().enumerate() {
            let txid = tx.compute_txid();
            let coinbase = tx.is_coinbase();
            if !coinbase {
                for (input, txin) in tx.input.iter().enumerate() {
                    let spent = txin.previous_output;
                    if let Some(coin) = self.coins.remove(&spent) {
                        self.unindex(&spent, &coin.output.script_pubkey);
                    }
                    self.spenders.insert(
                        spent,
                        Spender {
                            txid,
                            input: input as u32,
                        },
                    );
                }
            }
            self.locations.insert(
                txid,
                Location {
                    height,
                    position: position as u32,
                },
            );
            if height == 0 {
                continue;
            }
            for (vout, output) in tx.output.iter().enumerate() {
                if output.script_pubkey.is_op_return() {
                    continue;
                }
                let outpoint = OutPoint::new(txid, vout as u32);
                self.by_script
                    .entry(script_hash(&output.script_pubkey))
                    .or_default()
                    .insert(outpoint);
                self.coins.insert(
                    outpoint,
                    Coin {
                        output: output.clone(),
                        height,
                        coinbase,
                    },
                );
            }
        }
    }

    fn unindex(&mut self, outpoint: &OutPoint, script: &Script) {
        let hash = script_hash(script);
        if let Some(outpoints) = self.by_script.get_mut(&hash) {
            outpoints.remove(outpoint);
            if outpoints.is_empty() {
                self.by_script.remove(&hash);
            }
        }
    }

    pub fn coin(&self, outpoint: &OutPoint) -> Option<&Coin> {
        self.coins.get(outpoint)
    }

    /// The unspent outputs paying the script with this hash.
    pub fn coins_for(&self, hash: &ScriptHash) -> Vec<(OutPoint, &Coin)> {
        self.by_script
            .get(hash)
            .into_iter()
            .flatten()
            .filter_map(|outpoint| Some((*outpoint, self.coins.get(outpoint)?)))
            .collect()
    }

    pub fn location(&self, txid: &Txid) -> Option<Location> {
        self.locations.get(txid).copied()
    }

    pub fn spender(&self, outpoint: &OutPoint) -> Option<Spender> {
        self.spenders.get(outpoint).copied()
    }

    pub fn coin_count(&self) -> usize {
        self.coins.len()
    }
}

/// A transaction waiting to be mined.
#[derive(Clone, Debug)]
pub struct Entry {
    pub tx: Transaction,
    pub txid: Txid,
    pub fee: Amount,
    pub weight: Weight,
}

impl Entry {
    pub fn vsize(&self) -> u64 {
        self.weight.to_vbytes_ceil()
    }
}

/// Accepted transactions in the order they arrived. A transaction may spend
/// the outputs of one that arrived before it.
#[derive(Clone, Default)]
pub struct Mempool {
    entries: Vec<Entry>,
    positions: HashMap<Txid, usize>,
    spent: HashMap<OutPoint, Txid>,
}

impl Mempool {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, txid: &Txid) -> Option<&Entry> {
        self.positions.get(txid).map(|&index| &self.entries[index])
    }

    /// The mempool transaction spending `outpoint`, if any.
    pub fn spender(&self, outpoint: &OutPoint) -> Option<Txid> {
        self.spent.get(outpoint).copied()
    }

    /// An output created by a mempool transaction and not spent by another.
    pub fn output(&self, outpoint: &OutPoint) -> Option<&TxOut> {
        if self.spent.contains_key(outpoint) {
            return None;
        }
        self.get(&outpoint.txid)?
            .tx
            .output
            .get(outpoint.vout as usize)
    }

    pub fn add(&mut self, entry: Entry) {
        for input in &entry.tx.input {
            self.spent.insert(input.previous_output, entry.txid);
        }
        self.positions.insert(entry.txid, self.entries.len());
        self.entries.push(entry);
    }

    pub fn total_fee(&self) -> Amount {
        self.entries.iter().map(|entry| entry.fee).sum()
    }

    /// Remove every transaction, in arrival order.
    pub fn take(&mut self) -> Vec<Entry> {
        self.positions.clear();
        self.spent.clear();
        std::mem::take(&mut self.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::Chain;
    use bitcoin::absolute::LockTime;
    use bitcoin::{transaction, ScriptBuf, Sequence, TxIn, Witness};

    fn script(byte: u8) -> ScriptBuf {
        ScriptBuf::from_bytes(vec![0x51, byte])
    }

    fn spend(outpoint: OutPoint, outputs: Vec<TxOut>) -> Transaction {
        Transaction {
            version: transaction::Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: outpoint,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: outputs,
        }
    }

    fn output(value: u64, script: ScriptBuf) -> TxOut {
        TxOut {
            value: Amount::from_sat(value),
            script_pubkey: script,
        }
    }

    #[test]
    fn tracks_coins_spenders_and_scripts_across_blocks() {
        let mut chain = Chain::regtest();
        let mut ledger = Ledger::default();
        ledger.apply_block(0, chain.block(0).unwrap());
        assert_eq!(ledger.coin_count(), 0, "the genesis output is unspendable");

        chain.mine(Vec::new(), script(1), Amount::ZERO, 0);
        ledger.apply_block(1, chain.block(1).unwrap());
        let coinbase = chain.block(1).unwrap().txdata[0].compute_txid();
        let reward = OutPoint::new(coinbase, 0);
        let coin = ledger.coin(&reward).unwrap();
        assert!(coin.coinbase);
        assert_eq!(coin.height, 1);
        assert_eq!(ledger.coins_for(&script_hash(&script(1))).len(), 1);

        let payment = spend(
            reward,
            vec![
                output(1_000, script(2)),
                output(0, ScriptBuf::from_bytes(vec![0x6a, 0x01, 0x00])),
            ],
        );
        let txid = payment.compute_txid();
        chain.mine(vec![payment], script(1), Amount::ZERO, 0);
        ledger.apply_block(2, chain.block(2).unwrap());

        assert!(ledger.coin(&reward).is_none());
        assert_eq!(ledger.spender(&reward), Some(Spender { txid, input: 0 }));
        assert!(
            ledger.coins_for(&script_hash(&script(1))).len() == 1,
            "block 2's reward"
        );
        assert_eq!(ledger.coins_for(&script_hash(&script(2))).len(), 1);
        assert!(
            ledger.coin(&OutPoint::new(txid, 1)).is_none(),
            "an OP_RETURN output is not a coin"
        );
        assert_eq!(
            ledger.location(&txid),
            Some(Location {
                height: 2,
                position: 1
            })
        );
    }

    #[test]
    fn mempool_chains_and_drains_in_order() {
        let mut mempool = Mempool::default();
        let funding = OutPoint::new(Txid::all_zeros(), 0);
        let first = spend(funding, vec![output(900, script(1))]);
        let first_id = first.compute_txid();
        let second = spend(OutPoint::new(first_id, 0), vec![output(800, script(2))]);
        let second_id = second.compute_txid();

        for (tx, txid) in [(first, first_id), (second, second_id)] {
            mempool.add(Entry {
                weight: tx.weight(),
                tx,
                txid,
                fee: Amount::from_sat(100),
            });
        }

        assert_eq!(mempool.spender(&funding), Some(first_id));
        assert!(
            mempool.output(&OutPoint::new(first_id, 0)).is_none(),
            "spent by the second"
        );
        assert_eq!(
            mempool
                .output(&OutPoint::new(second_id, 0))
                .unwrap()
                .value
                .to_sat(),
            800
        );
        assert_eq!(mempool.total_fee(), Amount::from_sat(200));

        let taken: Vec<_> = mempool.take().into_iter().map(|entry| entry.txid).collect();
        assert_eq!(taken, vec![first_id, second_id]);
        assert!(mempool.is_empty() && mempool.spender(&funding).is_none());
    }
}
