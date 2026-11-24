//! A simulated regtest chain: an ordered list of blocks this process builds.
//!
//! The chain starts at Bitcoin's regtest genesis block. Each mined block has
//! a coinbase transaction that commits to its height, a correct merkle root
//! and witness commitment, and a header that satisfies regtest proof of work.
//!
//! What it does not do yet: transactions passed to [`Chain::mine`] are
//! included as given. There is no UTXO set, no script or signature
//! validation, no fee accounting, and no mempool.

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version};
use bitcoin::blockdata::constants::genesis_block;
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::script::PushBytesBuf;
use bitcoin::transaction;
use bitcoin::{
    Amount, Block, BlockHash, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn,
    TxMerkleNode, TxOut, Witness,
};

/// Regtest halves the block subsidy every 150 blocks.
const HALVING_INTERVAL: u32 = 150;
const INITIAL_SUBSIDY: Amount = Amount::from_sat(50 * 100_000_000);
/// Seconds between the timestamps of consecutive blocks.
const BLOCK_INTERVAL: u32 = 600;

/// The blocks of one simulated regtest chain.
#[derive(Clone)]
pub struct Chain {
    blocks: Vec<Block>,
}

impl Default for Chain {
    fn default() -> Self {
        Self::regtest()
    }
}

impl Chain {
    /// A chain holding only the regtest genesis block.
    pub fn regtest() -> Self {
        Self {
            blocks: vec![genesis_block(Network::Regtest)],
        }
    }

    /// The height of the newest block. The genesis block is height 0.
    pub fn height(&self) -> u32 {
        (self.blocks.len() - 1) as u32
    }

    /// The newest block.
    pub fn tip(&self) -> &Block {
        self.blocks
            .last()
            .expect("a chain always has its genesis block")
    }

    /// The block at `height`.
    pub fn block(&self, height: u32) -> Option<&Block> {
        self.blocks.get(height as usize)
    }

    /// The block at `height` in consensus serialization.
    pub fn block_bytes(&self, height: u32) -> Option<Vec<u8>> {
        self.block(height).map(serialize)
    }

    /// The hash of the block at `height`.
    pub fn block_hash(&self, height: u32) -> Option<BlockHash> {
        self.block(height).map(Block::block_hash)
    }

    /// Mine a block on the tip holding `transactions` in the order given,
    /// paying the block subsidy to `reward`. Returns the new height.
    pub fn mine(&mut self, transactions: Vec<Transaction>, reward: ScriptBuf) -> u32 {
        let height = self.height() + 1;
        let previous = self.tip().header;

        let mut txdata = Vec::with_capacity(transactions.len() + 1);
        txdata.push(coinbase(height, reward));
        txdata.extend(transactions);

        let mut block = Block {
            header: Header {
                version: Version::from_consensus(0x2000_0000),
                prev_blockhash: previous.block_hash(),
                merkle_root: TxMerkleNode::all_zeros(),
                time: previous.time + BLOCK_INTERVAL,
                bits: previous.bits,
                nonce: 0,
            },
            txdata,
        };
        commit_to_witnesses(&mut block);
        block.header.merkle_root = block
            .compute_merkle_root()
            .expect("a block with a coinbase has a merkle root");
        solve(&mut block.header);

        self.blocks.push(block);
        height
    }

    /// Drop every block above `height`. The genesis block always stays.
    pub fn truncate(&mut self, height: u32) {
        self.blocks.truncate(height as usize + 1);
    }
}

/// The block subsidy at `height`.
pub fn subsidy(height: u32) -> Amount {
    let halvings = height / HALVING_INTERVAL;
    if halvings >= 64 {
        Amount::ZERO
    } else {
        Amount::from_sat(INITIAL_SUBSIDY.to_sat() >> halvings)
    }
}

fn coinbase(height: u32, reward: ScriptBuf) -> Transaction {
    // BIP 34: the coinbase input script starts with the block height, encoded
    // as Bitcoin Core encodes it. A second push keeps the script above the
    // two-byte minimum at low heights.
    let script_sig = bitcoin::script::Builder::new()
        .push_int(i64::from(height))
        .push_slice(PushBytesBuf::try_from(b"labcoat".to_vec()).expect("a short push"))
        .into_script();

    Transaction {
        version: transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig,
            sequence: Sequence::MAX,
            // BIP 141: the witness reserved value.
            witness: Witness::from_slice(&[[0u8; 32]]),
        }],
        output: vec![TxOut {
            value: subsidy(height),
            script_pubkey: reward,
        }],
    }
}

/// Add the BIP 141 witness commitment to the coinbase transaction.
fn commit_to_witnesses(block: &mut Block) {
    let root = block
        .witness_root()
        .expect("a block with a coinbase has a witness root");
    let commitment = Block::compute_witness_commitment(&root, &[0u8; 32]);

    let mut script = vec![0x6a, 0x24, 0xaa, 0x21, 0xa9, 0xed];
    script.extend_from_slice(commitment.as_byte_array());
    block.txdata[0].output.push(TxOut {
        value: Amount::ZERO,
        script_pubkey: ScriptBuf::from_bytes(script),
    });
}

/// Find a nonce that satisfies the header's own target. On regtest about
/// every second nonce does.
fn solve(header: &mut Header) {
    let target = header.target();
    while header.validate_pow(target).is_err() {
        header.nonce = header.nonce.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::consensus::encode::deserialize;

    fn reward() -> ScriptBuf {
        // OP_TRUE: anyone can spend. Enough for blocks nobody validates yet.
        ScriptBuf::from_bytes(vec![0x51])
    }

    fn spend(previous: &Transaction) -> Transaction {
        Transaction {
            version: transaction::Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::new(previous.compute_txid(), 0),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::from_slice(&[b"witness data".to_vec()]),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(1_000),
                script_pubkey: reward(),
            }],
        }
    }

    #[test]
    fn starts_at_the_regtest_genesis_block() {
        let chain = Chain::regtest();
        assert_eq!(chain.height(), 0);
        assert_eq!(
            chain.block_hash(0).unwrap().to_string(),
            "0f9188f13cb7b2c71f2a335e3a4fc328bf5beb436012afca590b1a11466e2206"
        );
    }

    #[test]
    fn mines_blocks_that_pass_block_checks() {
        let mut chain = Chain::regtest();
        let first = chain.mine(Vec::new(), reward());
        let coinbase = chain.block(first).unwrap().txdata[0].clone();
        let second = chain.mine(vec![spend(&coinbase)], reward());

        assert_eq!((first, second), (1, 2));
        for height in 1..=2 {
            let block = chain.block(height).unwrap();
            let previous = chain.block(height - 1).unwrap();
            assert_eq!(block.header.prev_blockhash, previous.block_hash());
            assert!(block.header.time > previous.header.time);
            assert!(block.check_merkle_root());
            assert!(block.check_witness_commitment());
            assert!(block.header.validate_pow(block.header.target()).is_ok());
            assert!(block.txdata[0].is_coinbase());
        }
        assert_eq!(chain.block(2).unwrap().txdata.len(), 2);
    }

    #[test]
    fn commits_to_the_height_as_bitcoin_core_does() {
        let mut chain = Chain::regtest();
        for _ in 0..17 {
            chain.mine(Vec::new(), reward());
        }

        // Heights 1 to 16 are the single opcodes OP_1 to OP_16.
        let script = |height| {
            chain.block(height).unwrap().txdata[0].input[0]
                .script_sig
                .to_bytes()
        };
        assert_eq!(script(1)[0], 0x51);
        assert_eq!(script(16)[0], 0x60);
        // From 17 on the height is a data push.
        assert_eq!(script(17)[..2], [0x01, 17]);
        assert_eq!(chain.block(17).unwrap().bip34_block_height().unwrap(), 17);
    }

    #[test]
    fn serializes_blocks_for_the_indexer() {
        let mut chain = Chain::regtest();
        chain.mine(Vec::new(), reward());

        let bytes = chain.block_bytes(1).unwrap();
        let decoded: Block = deserialize(&bytes).unwrap();
        assert_eq!(decoded.block_hash(), chain.block_hash(1).unwrap());
        assert!(chain.block_bytes(2).is_none());
    }

    #[test]
    fn is_deterministic() {
        let mut one = Chain::regtest();
        let mut two = Chain::regtest();
        for _ in 0..3 {
            one.mine(Vec::new(), reward());
            two.mine(Vec::new(), reward());
        }
        assert_eq!(one.block_hash(3), two.block_hash(3));
    }

    #[test]
    fn truncates_but_keeps_genesis() {
        let mut chain = Chain::regtest();
        chain.mine(Vec::new(), reward());
        chain.mine(Vec::new(), reward());

        chain.truncate(1);
        assert_eq!(chain.height(), 1);
        chain.truncate(0);
        chain.truncate(0);
        assert_eq!(chain.height(), 0);
    }

    #[test]
    fn halves_the_subsidy_every_150_blocks() {
        assert_eq!(subsidy(1), Amount::from_sat(5_000_000_000));
        assert_eq!(subsidy(149), Amount::from_sat(5_000_000_000));
        assert_eq!(subsidy(150), Amount::from_sat(2_500_000_000));
        assert_eq!(subsidy(150 * 64), Amount::ZERO);
    }
}
