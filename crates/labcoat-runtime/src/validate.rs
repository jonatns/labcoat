//! Whether a transaction may enter the mempool.
//!
//! The checks follow Bitcoin Core's consensus rules and its default relay
//! policy, so a transaction that Labcoat Network accepts should also relay on
//! a public network. Rejections reuse Bitcoin Core's reason strings.
//!
//! Scripts are verified for the output types wallets create: pay-to-taproot
//! (key path and tapscript), pay-to-witness-pubkey-hash, pay-to-pubkey-hash,
//! and pay-to-witness-pubkey-hash nested in pay-to-script-hash. Tapscript is
//! executed by a small interpreter that covers the opcodes inscription-style
//! envelopes and signature checks use. Anything else is rejected with a
//! reason that names it, rather than accepted unchecked.

use std::sync::OnceLock;

use bitcoin::absolute::{Height, Time};
use bitcoin::hashes::{hash160, ripemd160, sha1, sha256, sha256d, Hash};
use bitcoin::opcodes::{all, Class, ClassifyContext};
use bitcoin::script::Instruction;
use bitcoin::secp256k1::{Message, Secp256k1, VerifyOnly};
use bitcoin::sighash::{Annex, Prevouts, SighashCache};
use bitcoin::taproot::{ControlBlock, LeafVersion, TapLeafHash};
use bitcoin::{
    ecdsa, taproot, Amount, OutPoint, PublicKey, Script, ScriptBuf, Transaction, TxOut, Weight,
    XOnlyPublicKey,
};

use crate::chain::Chain;
use crate::ledger::{Coin, Ledger, Mempool};

/// Blocks before a coinbase output can be spent.
pub const COINBASE_MATURITY: u32 = 100;
/// Bitcoin Core does not relay transactions heavier than this.
pub const MAX_STANDARD_WEIGHT: u64 = 400_000;
/// The minimum relay fee: one satoshi per virtual byte.
pub const MIN_RELAY_SATS_PER_VBYTE: u64 = 1;
const MAX_MONEY: Amount = Amount::MAX_MONEY;
/// Bitcoin Core rejects transactions smaller than this without witnesses.
const MIN_STANDARD_NON_WITNESS_SIZE: usize = 65;

const MAX_SCRIPT_ELEMENT: usize = 520;
const MAX_STACK: usize = 1_000;

/// Why a transaction was refused. `code` is the Bitcoin Core RPC error code.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct Rejection {
    pub code: i64,
    pub reason: String,
}

impl Rejection {
    fn invalid(reason: impl Into<String>) -> Self {
        Self {
            code: -26,
            reason: reason.into(),
        }
    }

    fn verify(reason: impl Into<String>) -> Self {
        Self {
            code: -25,
            reason: reason.into(),
        }
    }
}

/// A transaction that passed every check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub fee: Amount,
    pub weight: Weight,
}

/// What a transaction is checked against.
pub struct Context<'a> {
    pub chain: &'a Chain,
    pub ledger: &'a Ledger,
    pub mempool: &'a Mempool,
    /// The height of the next block: the one this transaction would join.
    pub next_height: u32,
}

impl Context<'_> {
    /// The median time past of the block before the next one: the time
    /// locks are measured against.
    fn lock_time(&self) -> u32 {
        self.chain.median_time_past(self.next_height - 1)
    }
}

/// Check `tx` against the chain and the mempool. `max_fee_rate` is the
/// highest fee rate, in satoshis per virtual byte, the caller allows;
/// `None` allows any.
pub fn check(
    tx: &Transaction,
    context: &Context,
    max_fee_rate: Option<u64>,
) -> Result<Accepted, Rejection> {
    check_shape(tx)?;

    let txid = tx.compute_txid();
    if context.mempool.get(&txid).is_some() {
        return Err(Rejection::invalid("txn-already-in-mempool"));
    }
    if context.ledger.location(&txid).is_some() {
        return Err(Rejection {
            code: -27,
            reason: "Transaction already in block chain".into(),
        });
    }

    let tip = Height::from_consensus(context.next_height - 1)
        .map_err(|_| Rejection::invalid("non-final"))?;
    let time = Time::from_consensus(context.lock_time().max(Time::MIN.to_consensus_u32()))
        .map_err(|_| Rejection::invalid("non-final"))?;
    if !tx.is_absolute_timelock_satisfied(tip, time) {
        return Err(Rejection::invalid("non-final"));
    }

    let coins = spent_coins(tx, context)?;
    if !sequence_locks_satisfied(tx, &coins, context) {
        return Err(Rejection::invalid("non-BIP68-final"));
    }
    let input_value: Amount = coins.iter().map(|coin| coin.output.value).sum();
    let output_value: Amount = tx.output.iter().map(|output| output.value).sum();
    let fee = input_value
        .checked_sub(output_value)
        .ok_or_else(|| Rejection::invalid("bad-txns-in-belowout"))?;

    let weight = tx.weight();
    let vsize = weight.to_vbytes_ceil();
    let required = vsize * MIN_RELAY_SATS_PER_VBYTE;
    if fee.to_sat() < required {
        return Err(Rejection::invalid(format!(
            "min relay fee not met, {} < {required}",
            fee.to_sat()
        )));
    }
    if let Some(max) = max_fee_rate {
        if fee.to_sat() > max.saturating_mul(vsize) {
            return Err(Rejection::verify(
                "Fee exceeds maximum configured by user (e.g. -maxtxfee, maxfeerate)",
            ));
        }
    }

    let prevouts: Vec<TxOut> = coins.into_iter().map(|coin| coin.output).collect();
    let mut cache = SighashCache::new(tx);
    for index in 0..tx.input.len() {
        verify_input(tx, index, &prevouts, &mut cache).map_err(|reason| {
            Rejection::invalid(format!(
                "mandatory-script-verify-flag-failed (input {index}: {reason})"
            ))
        })?;
    }

    Ok(Accepted { fee, weight })
}

/// Checks that need only the transaction itself.
fn check_shape(tx: &Transaction) -> Result<(), Rejection> {
    if tx.input.is_empty() {
        return Err(Rejection::invalid("bad-txns-vin-empty"));
    }
    if tx.output.is_empty() {
        return Err(Rejection::invalid("bad-txns-vout-empty"));
    }
    if tx.is_coinbase() {
        return Err(Rejection::invalid("coinbase"));
    }
    if tx.weight().to_wu() > MAX_STANDARD_WEIGHT {
        return Err(Rejection::invalid(format!(
            "tx-size: the transaction weighs {} weight units; Bitcoin Core relays at most {MAX_STANDARD_WEIGHT}",
            tx.weight().to_wu()
        )));
    }
    if tx.base_size() < MIN_STANDARD_NON_WITNESS_SIZE {
        return Err(Rejection::invalid("tx-size-small"));
    }

    let mut total = Amount::ZERO;
    for output in &tx.output {
        if output.value > MAX_MONEY {
            return Err(Rejection::invalid("bad-txns-vout-toolarge"));
        }
        total = total
            .checked_add(output.value)
            .filter(|total| *total <= MAX_MONEY)
            .ok_or_else(|| Rejection::invalid("bad-txns-txouttotal-toolarge"))?;
        if !output.script_pubkey.is_op_return()
            && output.value < output.script_pubkey.minimal_non_dust()
        {
            return Err(Rejection::invalid(format!(
                "dust: an output of {} sat is below the {} sat Bitcoin Core relays for its script type",
                output.value.to_sat(),
                output.script_pubkey.minimal_non_dust().to_sat()
            )));
        }
    }

    let mut seen = std::collections::HashSet::with_capacity(tx.input.len());
    if !tx
        .input
        .iter()
        .all(|input| seen.insert(input.previous_output))
    {
        return Err(Rejection::invalid("bad-txns-inputs-duplicate"));
    }
    Ok(())
}

/// The outputs `tx` spends, from the mempool or the chain.
fn spent_coins(tx: &Transaction, context: &Context) -> Result<Vec<Coin>, Rejection> {
    tx.input
        .iter()
        .map(|input| {
            let outpoint = input.previous_output;
            if context.mempool.spender(&outpoint).is_some() {
                return Err(Rejection::invalid("txn-mempool-conflict"));
            }
            let coin = coin(&outpoint, context)
                .ok_or_else(|| Rejection::verify("bad-txns-inputs-missingorspent"))?;
            if coin.coinbase && context.next_height - coin.height < COINBASE_MATURITY {
                return Err(Rejection::invalid(format!(
                    "bad-txns-premature-spend-of-coinbase, tried to spend coinbase at depth {}",
                    context.next_height - coin.height
                )));
            }
            Ok(coin)
        })
        .collect()
}

/// BIP 68: whether every input's relative lock has passed by the next
/// block, computed as Bitcoin Core's `CalculateSequenceLocks` does.
fn sequence_locks_satisfied(tx: &Transaction, coins: &[Coin], context: &Context) -> bool {
    const DISABLE: u32 = 1 << 31;
    const TIME_BASED: u32 = 1 << 22;
    const VALUE: u32 = 0xffff;
    const GRANULARITY: u32 = 9;

    if tx.version.0 < 2 {
        return true;
    }
    let mut min_height: i64 = -1;
    let mut min_time: i64 = -1;
    for (input, coin) in tx.input.iter().zip(coins) {
        let sequence = input.sequence.to_consensus_u32();
        if sequence & DISABLE != 0 {
            continue;
        }
        let value = i64::from(sequence & VALUE);
        if sequence & TIME_BASED != 0 {
            // Measured from the median time past of the block before the
            // one that confirmed the coin.
            let coin_time = i64::from(
                context
                    .chain
                    .median_time_past(coin.height.saturating_sub(1)),
            );
            min_time = min_time.max(coin_time + (value << GRANULARITY) - 1);
        } else {
            min_height = min_height.max(i64::from(coin.height) + value - 1);
        }
    }
    min_height < i64::from(context.next_height) && min_time < i64::from(context.lock_time())
}

fn coin(outpoint: &OutPoint, context: &Context) -> Option<Coin> {
    if let Some(output) = context.mempool.output(outpoint) {
        return Some(Coin {
            output: output.clone(),
            height: context.next_height,
            coinbase: false,
        });
    }
    context.ledger.coin(outpoint).cloned()
}

fn secp() -> &'static Secp256k1<VerifyOnly> {
    static SECP: OnceLock<Secp256k1<VerifyOnly>> = OnceLock::new();
    SECP.get_or_init(Secp256k1::verification_only)
}

/// Verify the input at `index` against the output it spends.
fn verify_input(
    tx: &Transaction,
    index: usize,
    prevouts: &[TxOut],
    cache: &mut SighashCache<&Transaction>,
) -> Result<(), String> {
    let input = &tx.input[index];
    let script = &prevouts[index].script_pubkey;
    let bytes = script.as_bytes();

    if script.is_p2tr() {
        no_script_sig(input.script_sig.as_script())?;
        let key = XOnlyPublicKey::from_slice(&bytes[2..34])
            .map_err(|_| "the output's taproot key is not a valid point")?;
        return verify_taproot(tx, index, prevouts, key, cache);
    }
    if script.is_p2wpkh() {
        no_script_sig(input.script_sig.as_script())?;
        return verify_p2wpkh(
            tx,
            index,
            script,
            &bytes[2..22],
            prevouts[index].value,
            cache,
        );
    }
    if script.is_p2pkh() {
        if !input.witness.is_empty() {
            return Err("a pay-to-pubkey-hash input carries a witness".into());
        }
        let pushes = pushes(input.script_sig.as_script())?;
        let [signature, key] = pushes.as_slice() else {
            return Err("a pay-to-pubkey-hash input needs a signature and a public key".into());
        };
        let key = PublicKey::from_slice(key).map_err(|_| "the public key is not valid")?;
        if key.pubkey_hash().as_byte_array() != &bytes[3..23] {
            return Err("the public key does not match the output's hash".into());
        }
        let signature = ecdsa::Signature::from_slice(signature)
            .map_err(|_| "the signature is not valid DER")?;
        let hash = cache
            .legacy_signature_hash(index, script, signature.sighash_type.to_u32())
            .map_err(|error| error.to_string())?;
        return verify_ecdsa(hash.to_byte_array(), &signature, &key);
    }
    if script.is_p2sh() {
        let pushes = pushes(input.script_sig.as_script())?;
        let [redeem] = pushes.as_slice() else {
            return Err(
                "Labcoat Network verifies pay-to-script-hash only for nested pay-to-witness-pubkey-hash".into(),
            );
        };
        if hash160::Hash::hash(redeem).as_byte_array() != &bytes[2..22] {
            return Err("the redeem script does not match the output's hash".into());
        }
        let redeem = ScriptBuf::from_bytes(redeem.clone());
        if !redeem.is_p2wpkh() {
            return Err(
                "Labcoat Network verifies pay-to-script-hash only for nested pay-to-witness-pubkey-hash".into(),
            );
        }
        let program = redeem.as_bytes()[2..22].to_vec();
        return verify_p2wpkh(tx, index, &redeem, &program, prevouts[index].value, cache);
    }
    if script.is_p2wsh() {
        return Err("Labcoat Network does not verify pay-to-witness-script-hash spends yet".into());
    }
    Err(format!(
        "Labcoat Network does not verify spends of the script {script}"
    ))
}

fn no_script_sig(script_sig: &Script) -> Result<(), String> {
    if script_sig.is_empty() {
        Ok(())
    } else {
        Err("a witness input must have an empty scriptSig".into())
    }
}

fn pushes(script: &Script) -> Result<Vec<Vec<u8>>, String> {
    script
        .instructions()
        .map(|instruction| match instruction {
            Ok(Instruction::PushBytes(bytes)) => Ok(bytes.as_bytes().to_vec()),
            _ => Err("the scriptSig must only push data".to_owned()),
        })
        .collect()
}

fn verify_p2wpkh(
    tx: &Transaction,
    index: usize,
    script_code_source: &Script,
    program: &[u8],
    value: Amount,
    cache: &mut SighashCache<&Transaction>,
) -> Result<(), String> {
    let witness: Vec<&[u8]> = tx.input[index].witness.iter().collect();
    let [signature, key] = witness.as_slice() else {
        return Err(
            "a pay-to-witness-pubkey-hash witness needs a signature and a public key".into(),
        );
    };
    let key = PublicKey::from_slice(key).map_err(|_| "the public key is not valid")?;
    if !key.compressed {
        return Err("witness public keys must be compressed".into());
    }
    if key.pubkey_hash().as_byte_array() != program {
        return Err("the public key does not match the witness program".into());
    }
    let signature =
        ecdsa::Signature::from_slice(signature).map_err(|_| "the signature is not valid DER")?;
    let hash = cache
        .p2wpkh_signature_hash(index, script_code_source, value, signature.sighash_type)
        .map_err(|error| error.to_string())?;
    verify_ecdsa(hash.to_byte_array(), &signature, &key)
}

fn verify_ecdsa(
    digest: [u8; 32],
    signature: &ecdsa::Signature,
    key: &PublicKey,
) -> Result<(), String> {
    secp()
        .verify_ecdsa(
            &Message::from_digest(digest),
            &signature.signature,
            &key.inner,
        )
        .map_err(|_| "Signature must be zero for failed CHECK(MULTI)SIG operation".to_owned())
}

fn verify_taproot(
    tx: &Transaction,
    index: usize,
    prevouts: &[TxOut],
    output_key: XOnlyPublicKey,
    cache: &mut SighashCache<&Transaction>,
) -> Result<(), String> {
    let mut stack: Vec<&[u8]> = tx.input[index].witness.iter().collect();
    let annex = match stack.last() {
        Some(last) if stack.len() >= 2 && last.first() == Some(&0x50) => Some(
            Annex::new(stack.pop().expect("checked above")).map_err(|error| error.to_string())?,
        ),
        _ => None,
    };
    let prevouts = Prevouts::All(prevouts);

    match stack.len() {
        0 => Err("Witness program was passed an empty witness".into()),
        1 => {
            let signature = taproot::Signature::from_slice(stack[0])
                .map_err(|_| "Invalid Schnorr signature size or hash type")?;
            let hash = cache
                .taproot_signature_hash(index, &prevouts, annex, None, signature.sighash_type)
                .map_err(|error| error.to_string())?;
            secp()
                .verify_schnorr(
                    &signature.signature,
                    &Message::from_digest(hash.to_byte_array()),
                    &output_key,
                )
                .map_err(|_| "Invalid Schnorr signature".to_owned())
        }
        _ => {
            let control = ControlBlock::decode(stack.pop().expect("two or more items"))
                .map_err(|_| "Invalid Taproot control block size")?;
            let script = ScriptBuf::from_bytes(stack.pop().expect("two or more items").to_vec());
            if !control.verify_taproot_commitment(secp(), output_key, &script) {
                return Err("Witness program hash mismatch".into());
            }
            if control.leaf_version != LeafVersion::TapScript {
                return Err("Taproot version reserved for soft-fork upgrades".into());
            }
            let leaf = TapLeafHash::from_script(&script, LeafVersion::TapScript);
            let mut checksig = |signature: &[u8], key: &[u8]| -> Result<bool, String> {
                if signature.is_empty() {
                    return Ok(false);
                }
                if key.len() != 32 {
                    // An unknown public key type succeeds, as BIP 342 says.
                    return Ok(true);
                }
                let key = XOnlyPublicKey::from_slice(key)
                    .map_err(|_| "Public key is neither compressed or uncompressed")?;
                let signature = taproot::Signature::from_slice(signature)
                    .map_err(|_| "Invalid Schnorr signature size or hash type")?;
                let hash = cache
                    .taproot_signature_hash(
                        index,
                        &prevouts,
                        annex.clone(),
                        Some((leaf, u32::MAX)),
                        signature.sighash_type,
                    )
                    .map_err(|error| error.to_string())?;
                secp()
                    .verify_schnorr(
                        &signature.signature,
                        &Message::from_digest(hash.to_byte_array()),
                        &key,
                    )
                    .map_err(|_| "Invalid Schnorr signature".to_owned())?;
                Ok(true)
            };
            let stack = stack.into_iter().map(<[u8]>::to_vec).collect();
            run_tapscript(&script, stack, &mut checksig)
        }
    }
}

type CheckSig<'a> = dyn FnMut(&[u8], &[u8]) -> Result<bool, String> + 'a;

/// Execute a tapscript leaf with the witness stack, as BIP 342 defines it,
/// for the opcodes listed below. Any other opcode is refused by name.
fn run_tapscript(
    script: &Script,
    mut stack: Vec<Vec<u8>>,
    checksig: &mut CheckSig,
) -> Result<(), String> {
    let mut instructions = Vec::new();
    for instruction in script.instructions() {
        let instruction = instruction.map_err(|_| "Opcode missing or not understood")?;
        if let Instruction::Op(op) = instruction {
            if op.classify(ClassifyContext::TapScript) == Class::SuccessOp {
                // Any OP_SUCCESS makes the script succeed (BIP 342).
                return Ok(());
            }
        }
        instructions.push(instruction);
    }
    if stack.iter().any(|item| item.len() > MAX_SCRIPT_ELEMENT) {
        return Err("Push value size limit exceeded".into());
    }

    let mut alt: Vec<Vec<u8>> = Vec::new();
    let mut branches: Vec<bool> = Vec::new();
    for instruction in instructions {
        let executing = branches.iter().all(|&taken| taken);
        let op = match instruction {
            Instruction::PushBytes(bytes) => {
                if executing {
                    if bytes.len() > MAX_SCRIPT_ELEMENT {
                        return Err("Push value size limit exceeded".into());
                    }
                    stack.push(bytes.as_bytes().to_vec());
                }
                continue;
            }
            Instruction::Op(op) => op,
        };

        match op {
            all::OP_IF | all::OP_NOTIF => {
                let mut taken = false;
                if executing {
                    let condition = pop(&mut stack)?;
                    if condition.len() > 1 || (condition.len() == 1 && condition[0] != 1) {
                        return Err("OP_IF/NOTIF argument must be minimal in tapscript".into());
                    }
                    taken = (condition.len() == 1) == (op == all::OP_IF);
                }
                branches.push(taken);
            }
            all::OP_ELSE => {
                let last = branches.last_mut().ok_or("Invalid OP_IF construction")?;
                *last = !*last;
            }
            all::OP_ENDIF => {
                branches.pop().ok_or("Invalid OP_IF construction")?;
            }
            _ if !executing => {}
            _ => execute(op, &mut stack, &mut alt, checksig)?,
        }
        if stack.len() + alt.len() > MAX_STACK {
            return Err("Stack size limit exceeded".into());
        }
    }

    if !branches.is_empty() {
        return Err("Invalid OP_IF construction".into());
    }
    if stack.len() != 1 {
        return Err("Stack size must be exactly one after execution".into());
    }
    if !truthy(&stack[0]) {
        return Err(
            "Script evaluated without error but finished with a false/empty top stack element"
                .into(),
        );
    }
    Ok(())
}

fn execute(
    op: bitcoin::Opcode,
    stack: &mut Vec<Vec<u8>>,
    alt: &mut Vec<Vec<u8>>,
    checksig: &mut CheckSig,
) -> Result<(), String> {
    let code = op.to_u8();
    match op {
        // Numbers.
        all::OP_PUSHNUM_NEG1 => stack.push(encode(-1)),
        _ if (all::OP_PUSHNUM_1.to_u8()..=all::OP_PUSHNUM_16.to_u8()).contains(&code) => {
            stack.push(encode(i64::from(code - all::OP_PUSHNUM_1.to_u8() + 1)))
        }
        all::OP_NOP
        | all::OP_NOP1
        | all::OP_NOP4
        | all::OP_NOP5
        | all::OP_NOP6
        | all::OP_NOP7
        | all::OP_NOP8
        | all::OP_NOP9
        | all::OP_NOP10 => {}
        all::OP_VERIFY => verify(&mut *stack, "Script failed an OP_VERIFY operation")?,
        all::OP_RETURN => return Err("OP_RETURN was encountered".into()),

        // Stack.
        all::OP_TOALTSTACK => {
            let item = pop(stack)?;
            alt.push(item);
        }
        all::OP_FROMALTSTACK => {
            let item = alt
                .pop()
                .ok_or("Operation not valid with the current altstack size")?;
            stack.push(item);
        }
        all::OP_DROP => {
            pop(stack)?;
        }
        all::OP_2DROP => {
            pop(stack)?;
            pop(stack)?;
        }
        all::OP_DUP => stack.push(peek(stack, 0)?.clone()),
        all::OP_2DUP => {
            let (a, b) = (peek(stack, 1)?.clone(), peek(stack, 0)?.clone());
            stack.extend([a, b]);
        }
        all::OP_IFDUP => {
            let top = peek(stack, 0)?.clone();
            if truthy(&top) {
                stack.push(top);
            }
        }
        all::OP_DEPTH => stack.push(encode(stack.len() as i64)),
        all::OP_NIP => {
            let top = pop(stack)?;
            pop(stack)?;
            stack.push(top);
        }
        all::OP_OVER => stack.push(peek(stack, 1)?.clone()),
        all::OP_SWAP => {
            let length = stack.len();
            if length < 2 {
                return Err(STACK_TOO_SMALL.into());
            }
            stack.swap(length - 1, length - 2);
        }
        all::OP_ROT => {
            let length = stack.len();
            if length < 3 {
                return Err(STACK_TOO_SMALL.into());
            }
            let item = stack.remove(length - 3);
            stack.push(item);
        }
        all::OP_TUCK => {
            let top = peek(stack, 0)?.clone();
            let length = stack.len();
            if length < 2 {
                return Err(STACK_TOO_SMALL.into());
            }
            stack.insert(length - 2, top);
        }
        all::OP_SIZE => {
            let size = peek(stack, 0)?.len() as i64;
            stack.push(encode(size));
        }

        // Comparison.
        all::OP_EQUAL | all::OP_EQUALVERIFY => {
            let (b, a) = (pop(stack)?, pop(stack)?);
            stack.push(boolean(a == b));
            if op == all::OP_EQUALVERIFY {
                verify(stack, "Script failed an OP_EQUALVERIFY operation")?;
            }
        }

        // Arithmetic.
        all::OP_1ADD
        | all::OP_1SUB
        | all::OP_NEGATE
        | all::OP_ABS
        | all::OP_NOT
        | all::OP_0NOTEQUAL => {
            let a = number(&pop(stack)?)?;
            stack.push(match op {
                all::OP_1ADD => encode(a + 1),
                all::OP_1SUB => encode(a - 1),
                all::OP_NEGATE => encode(-a),
                all::OP_ABS => encode(a.abs()),
                all::OP_NOT => boolean(a == 0),
                _ => boolean(a != 0),
            });
        }
        all::OP_ADD
        | all::OP_SUB
        | all::OP_BOOLAND
        | all::OP_BOOLOR
        | all::OP_NUMEQUAL
        | all::OP_NUMEQUALVERIFY
        | all::OP_NUMNOTEQUAL
        | all::OP_LESSTHAN
        | all::OP_GREATERTHAN
        | all::OP_LESSTHANOREQUAL
        | all::OP_GREATERTHANOREQUAL
        | all::OP_MIN
        | all::OP_MAX => {
            let b = number(&pop(stack)?)?;
            let a = number(&pop(stack)?)?;
            stack.push(match op {
                all::OP_ADD => encode(a + b),
                all::OP_SUB => encode(a - b),
                all::OP_BOOLAND => boolean(a != 0 && b != 0),
                all::OP_BOOLOR => boolean(a != 0 || b != 0),
                all::OP_NUMEQUAL | all::OP_NUMEQUALVERIFY => boolean(a == b),
                all::OP_NUMNOTEQUAL => boolean(a != b),
                all::OP_LESSTHAN => boolean(a < b),
                all::OP_GREATERTHAN => boolean(a > b),
                all::OP_LESSTHANOREQUAL => boolean(a <= b),
                all::OP_GREATERTHANOREQUAL => boolean(a >= b),
                all::OP_MIN => encode(a.min(b)),
                _ => encode(a.max(b)),
            });
            if op == all::OP_NUMEQUALVERIFY {
                verify(stack, "Script failed an OP_NUMEQUALVERIFY operation")?;
            }
        }
        all::OP_WITHIN => {
            let max = number(&pop(stack)?)?;
            let min = number(&pop(stack)?)?;
            let x = number(&pop(stack)?)?;
            stack.push(boolean(min <= x && x < max));
        }

        // Cryptography.
        all::OP_RIPEMD160 => {
            let item = pop(stack)?;
            stack.push(ripemd160::Hash::hash(&item).to_byte_array().to_vec());
        }
        all::OP_SHA1 => {
            let item = pop(stack)?;
            stack.push(sha1::Hash::hash(&item).to_byte_array().to_vec());
        }
        all::OP_SHA256 => {
            let item = pop(stack)?;
            stack.push(sha256::Hash::hash(&item).to_byte_array().to_vec());
        }
        all::OP_HASH160 => {
            let item = pop(stack)?;
            stack.push(hash160::Hash::hash(&item).to_byte_array().to_vec());
        }
        all::OP_HASH256 => {
            let item = pop(stack)?;
            stack.push(sha256d::Hash::hash(&item).to_byte_array().to_vec());
        }
        all::OP_CHECKSIG | all::OP_CHECKSIGVERIFY => {
            let key = pop(stack)?;
            let signature = pop(stack)?;
            if key.is_empty() {
                return Err("Public key is neither compressed or uncompressed".into());
            }
            let valid = checksig(&signature, &key)?;
            if op == all::OP_CHECKSIGVERIFY {
                if !valid {
                    return Err("Script failed an OP_CHECKSIGVERIFY operation".into());
                }
            } else {
                stack.push(boolean(valid));
            }
        }
        all::OP_CHECKSIGADD => {
            let key = pop(stack)?;
            let count = number(&pop(stack)?)?;
            let signature = pop(stack)?;
            if key.is_empty() {
                return Err("Public key is neither compressed or uncompressed".into());
            }
            let valid = checksig(&signature, &key)?;
            stack.push(encode(count + i64::from(valid)));
        }
        all::OP_CHECKMULTISIG | all::OP_CHECKMULTISIGVERIFY => {
            return Err("OP_CHECKMULTISIG(VERIFY) is not available in tapscript".into())
        }
        _ => {
            return Err(format!(
                "the script uses {op}, which Labcoat Network does not verify yet"
            ))
        }
    }
    Ok(())
}

const STACK_TOO_SMALL: &str = "Operation not valid with the current stack size";

fn pop(stack: &mut Vec<Vec<u8>>) -> Result<Vec<u8>, String> {
    stack.pop().ok_or_else(|| STACK_TOO_SMALL.to_owned())
}

fn peek(stack: &[Vec<u8>], depth: usize) -> Result<&Vec<u8>, String> {
    stack
        .len()
        .checked_sub(depth + 1)
        .map(|index| &stack[index])
        .ok_or_else(|| STACK_TOO_SMALL.to_owned())
}

fn verify(stack: &mut Vec<Vec<u8>>, failure: &str) -> Result<(), String> {
    if truthy(&pop(stack)?) {
        Ok(())
    } else {
        Err(failure.to_owned())
    }
}

/// Script's notion of true: any non-zero value, where negative zero is zero.
fn truthy(item: &[u8]) -> bool {
    item.iter()
        .enumerate()
        .any(|(index, &byte)| byte != 0 && !(index == item.len() - 1 && byte == 0x80))
}

fn boolean(value: bool) -> Vec<u8> {
    if value {
        vec![1]
    } else {
        Vec::new()
    }
}

/// Decode a script number of at most four bytes.
fn number(item: &[u8]) -> Result<i64, String> {
    if item.len() > 4 {
        return Err("Script number overflow".into());
    }
    let Some((&last, _)) = item.split_last() else {
        return Ok(0);
    };
    let mut value = 0i64;
    for (index, &byte) in item.iter().enumerate() {
        value |= i64::from(byte) << (8 * index);
    }
    if last & 0x80 != 0 {
        value &= !(0x80i64 << (8 * (item.len() - 1)));
        value = -value;
    }
    Ok(value)
}

fn encode(value: i64) -> Vec<u8> {
    if value == 0 {
        return Vec::new();
    }
    let negative = value < 0;
    let mut magnitude = value.unsigned_abs();
    let mut bytes = Vec::new();
    while magnitude > 0 {
        bytes.push((magnitude & 0xff) as u8);
        magnitude >>= 8;
    }
    if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        bytes.push(if negative { 0x80 } else { 0 });
    } else if negative {
        *bytes.last_mut().expect("non-zero") |= 0x80;
    }
    bytes
}

#[cfg(test)]
mod tests;
