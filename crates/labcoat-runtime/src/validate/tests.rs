use super::*;
use crate::chain::Chain;
use crate::ledger::Entry;
use bitcoin::absolute::LockTime;
use bitcoin::key::{Keypair, TapTweak};
use bitcoin::script::Builder;
use bitcoin::secp256k1::{All, SecretKey};
use bitcoin::sighash::{EcdsaSighashType, TapSighashType};
use bitcoin::taproot::TaprootBuilder;
use bitcoin::{transaction, CompressedPublicKey, Sequence, TxIn, Txid, Witness};

fn secp() -> Secp256k1<All> {
    Secp256k1::new()
}

fn secret(byte: u8) -> SecretKey {
    SecretKey::from_slice(&[byte; 32]).unwrap()
}

/// A chain whose block 1 pays `outputs` from a transaction nobody checks.
struct Fixture {
    ledger: Ledger,
    mempool: Mempool,
    funding: Txid,
}

impl Fixture {
    fn new(outputs: Vec<TxOut>) -> Self {
        let funding = Transaction {
            version: transaction::Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![input(OutPoint::new(Txid::all_zeros(), 7))],
            output: outputs,
        };
        let txid = funding.compute_txid();
        let mut chain = Chain::regtest();
        chain.mine(
            vec![funding],
            ScriptBuf::from_bytes(vec![0x51]),
            Amount::ZERO,
            0,
        );
        let mut ledger = Ledger::default();
        ledger.apply_block(0, chain.block(0).unwrap());
        ledger.apply_block(1, chain.block(1).unwrap());
        Self {
            ledger,
            mempool: Mempool::default(),
            funding: txid,
        }
    }

    fn check(&self, tx: &Transaction) -> Result<Accepted, Rejection> {
        check(
            tx,
            &Context {
                ledger: &self.ledger,
                mempool: &self.mempool,
                next_height: 2,
                tip_time: 1_700_000_000,
            },
            None,
        )
    }

    fn outpoint(&self, vout: u32) -> OutPoint {
        OutPoint::new(self.funding, vout)
    }
}

fn input(previous_output: OutPoint) -> TxIn {
    TxIn {
        previous_output,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    }
}

fn pay(value: u64) -> TxOut {
    TxOut {
        value: Amount::from_sat(value),
        script_pubkey: ScriptBuf::new_p2tr(&secp(), secret(9).x_only_public_key(&secp()).0, None),
    }
}

fn spending(outpoints: &[OutPoint], value: u64) -> Transaction {
    Transaction {
        version: transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: outpoints.iter().copied().map(input).collect(),
        output: vec![pay(value)],
    }
}

fn funded(script: ScriptBuf, value: u64) -> TxOut {
    TxOut {
        value: Amount::from_sat(value),
        script_pubkey: script,
    }
}

fn sign_key_path(tx: &mut Transaction, index: usize, prevouts: &[TxOut], key: &SecretKey) {
    let secp = secp();
    let keypair = Keypair::from_secret_key(&secp, key).tap_tweak(&secp, None);
    let hash = SighashCache::new(&*tx)
        .taproot_key_spend_signature_hash(index, &Prevouts::All(prevouts), TapSighashType::Default)
        .unwrap();
    let signature = secp.sign_schnorr_no_aux_rand(
        &Message::from_digest(hash.to_byte_array()),
        &keypair.to_keypair(),
    );
    tx.input[index].witness = Witness::p2tr_key_spend(&taproot::Signature {
        signature,
        sighash_type: TapSighashType::Default,
    });
}

fn p2tr(key: &SecretKey) -> ScriptBuf {
    let secp = secp();
    ScriptBuf::new_p2tr(&secp, key.x_only_public_key(&secp).0, None)
}

#[test]
fn accepts_a_signed_key_path_spend_and_reports_its_fee() {
    let key = secret(1);
    let fixture = Fixture::new(vec![funded(p2tr(&key), 100_000)]);
    let prevouts = [funded(p2tr(&key), 100_000)];
    let mut tx = spending(&[fixture.outpoint(0)], 99_000);
    sign_key_path(&mut tx, 0, &prevouts, &key);

    let accepted = fixture.check(&tx).unwrap();
    assert_eq!(accepted.fee, Amount::from_sat(1_000));
    assert_eq!(accepted.weight, tx.weight());
}

#[test]
fn rejects_a_key_path_signature_by_another_key() {
    let fixture = Fixture::new(vec![funded(p2tr(&secret(1)), 100_000)]);
    let prevouts = [funded(p2tr(&secret(1)), 100_000)];
    let mut tx = spending(&[fixture.outpoint(0)], 99_000);
    sign_key_path(&mut tx, 0, &prevouts, &secret(2));

    let rejection = fixture.check(&tx).unwrap_err();
    assert_eq!(rejection.code, -26);
    assert!(
        rejection.reason.contains("Invalid Schnorr signature"),
        "{rejection}"
    );
}

#[test]
fn rejects_a_signature_over_different_outputs() {
    let key = secret(1);
    let fixture = Fixture::new(vec![funded(p2tr(&key), 100_000)]);
    let prevouts = [funded(p2tr(&key), 100_000)];
    let mut tx = spending(&[fixture.outpoint(0)], 99_000);
    sign_key_path(&mut tx, 0, &prevouts, &key);
    tx.output[0].value = Amount::from_sat(98_000);

    assert!(fixture.check(&tx).is_err());
}

/// An envelope like the one Alkanes deploys use: a key check followed by
/// data that is never executed.
fn envelope(key: &XOnlyPublicKey) -> ScriptBuf {
    Builder::new()
        .push_x_only_key(key)
        .push_opcode(all::OP_CHECKSIG)
        .push_opcode(all::OP_PUSHBYTES_0)
        .push_opcode(all::OP_IF)
        .push_slice(b"BIN")
        .push_slice(bitcoin::script::PushBytesBuf::try_from(vec![7u8; 520]).unwrap())
        .push_slice(bitcoin::script::PushBytesBuf::try_from(vec![8u8; 300]).unwrap())
        .push_opcode(all::OP_ENDIF)
        .into_script()
}

fn script_path_fixture(script: &ScriptBuf) -> (Fixture, TxOut, ControlBlock) {
    let secp = secp();
    let internal = secret(3).x_only_public_key(&secp).0;
    let info = TaprootBuilder::new()
        .add_leaf(0, script.clone())
        .unwrap()
        .finalize(&secp, internal)
        .unwrap();
    let output = funded(ScriptBuf::new_p2tr_tweaked(info.output_key()), 50_000);
    let control = info
        .control_block(&(script.clone(), LeafVersion::TapScript))
        .unwrap();
    (Fixture::new(vec![output.clone()]), output, control)
}

fn sign_script_path(
    tx: &Transaction,
    prevouts: &[TxOut],
    script: &Script,
    key: &SecretKey,
) -> Vec<u8> {
    let secp = secp();
    let leaf = TapLeafHash::from_script(script, LeafVersion::TapScript);
    let hash = SighashCache::new(tx)
        .taproot_script_spend_signature_hash(
            0,
            &Prevouts::All(prevouts),
            leaf,
            TapSighashType::Default,
        )
        .unwrap();
    let keypair = Keypair::from_secret_key(&secp, key);
    secp.sign_schnorr_no_aux_rand(&Message::from_digest(hash.to_byte_array()), &keypair)
        .as_ref()
        .to_vec()
}

#[test]
fn accepts_a_signed_envelope_reveal() {
    let key = secret(4);
    let script = envelope(&key.x_only_public_key(&secp()).0);
    let (fixture, output, control) = script_path_fixture(&script);
    let mut tx = spending(&[fixture.outpoint(0)], 45_000);
    let signature = sign_script_path(&tx, &[output], &script, &key);
    tx.input[0].witness = Witness::from_slice(&[signature, script.to_bytes(), control.serialize()]);

    fixture.check(&tx).unwrap();
}

#[test]
fn rejects_an_envelope_signed_by_another_key_or_with_a_wrong_control_block() {
    let key = secret(4);
    let script = envelope(&key.x_only_public_key(&secp()).0);
    let (fixture, output, control) = script_path_fixture(&script);

    let mut tx = spending(&[fixture.outpoint(0)], 45_000);
    let signature = sign_script_path(&tx, std::slice::from_ref(&output), &script, &secret(5));
    tx.input[0].witness = Witness::from_slice(&[signature, script.to_bytes(), control.serialize()]);
    assert!(fixture
        .check(&tx)
        .unwrap_err()
        .reason
        .contains("Invalid Schnorr signature"));

    let other = envelope(&secret(5).x_only_public_key(&secp()).0);
    let mut tx = spending(&[fixture.outpoint(0)], 45_000);
    let signature = sign_script_path(&tx, &[output], &other, &secret(5));
    tx.input[0].witness = Witness::from_slice(&[signature, other.to_bytes(), control.serialize()]);
    assert!(fixture
        .check(&tx)
        .unwrap_err()
        .reason
        .contains("Witness program hash mismatch"));
}

#[test]
fn accepts_a_signed_p2wpkh_spend_and_rejects_a_bad_one() {
    let secp = secp();
    let key = secret(6);
    let public = CompressedPublicKey(key.public_key(&secp));
    let script = ScriptBuf::new_p2wpkh(&public.wpubkey_hash());
    let fixture = Fixture::new(vec![funded(script.clone(), 80_000)]);

    let sign = |tx: &mut Transaction, signer: &SecretKey| {
        let hash = SighashCache::new(&*tx)
            .p2wpkh_signature_hash(0, &script, Amount::from_sat(80_000), EcdsaSighashType::All)
            .unwrap();
        let signature = ecdsa::Signature::sighash_all(
            secp.sign_ecdsa(&Message::from_digest(hash.to_byte_array()), signer),
        );
        tx.input[0].witness = Witness::p2wpkh(&signature, &public.0);
    };

    let mut tx = spending(&[fixture.outpoint(0)], 79_000);
    sign(&mut tx, &key);
    fixture.check(&tx).unwrap();

    let mut forged = spending(&[fixture.outpoint(0)], 79_000);
    sign(&mut forged, &secret(7));
    assert!(fixture.check(&forged).is_err());
}

#[test]
fn accepts_signed_p2pkh_and_nested_p2wpkh_spends() {
    let secp = secp();
    let key = secret(8);
    let public = bitcoin::PublicKey::new(key.public_key(&secp));
    let compressed = CompressedPublicKey(key.public_key(&secp));
    let p2pkh = ScriptBuf::new_p2pkh(&public.pubkey_hash());
    let redeem = ScriptBuf::new_p2wpkh(&compressed.wpubkey_hash());
    let p2sh = ScriptBuf::new_p2sh(&redeem.script_hash());
    let fixture = Fixture::new(vec![funded(p2pkh.clone(), 60_000), funded(p2sh, 60_000)]);

    let mut legacy = spending(&[fixture.outpoint(0)], 59_000);
    let hash = SighashCache::new(&legacy)
        .legacy_signature_hash(0, &p2pkh, EcdsaSighashType::All.to_u32())
        .unwrap();
    let signature = ecdsa::Signature::sighash_all(
        secp.sign_ecdsa(&Message::from_digest(hash.to_byte_array()), &key),
    );
    legacy.input[0].script_sig = Builder::new()
        .push_slice(bitcoin::script::PushBytesBuf::try_from(signature.to_vec()).unwrap())
        .push_key(&public)
        .into_script();
    fixture.check(&legacy).unwrap();

    let mut nested = spending(&[fixture.outpoint(1)], 59_000);
    let hash = SighashCache::new(&nested)
        .p2wpkh_signature_hash(0, &redeem, Amount::from_sat(60_000), EcdsaSighashType::All)
        .unwrap();
    let signature = ecdsa::Signature::sighash_all(
        secp.sign_ecdsa(&Message::from_digest(hash.to_byte_array()), &key),
    );
    nested.input[0].script_sig = Builder::new()
        .push_slice(bitcoin::script::PushBytesBuf::try_from(redeem.to_bytes()).unwrap())
        .into_script();
    nested.input[0].witness = Witness::p2wpkh(&signature, &compressed.0);
    fixture.check(&nested).unwrap();
}

#[test]
fn applies_relay_policy() {
    let key = secret(1);
    let fixture = Fixture::new(vec![funded(p2tr(&key), 100_000)]);
    let prevouts = [funded(p2tr(&key), 100_000)];

    let mut free = spending(&[fixture.outpoint(0)], 100_000);
    sign_key_path(&mut free, 0, &prevouts, &key);
    assert!(fixture
        .check(&free)
        .unwrap_err()
        .reason
        .starts_with("min relay fee not met"));

    let mut dust = spending(&[fixture.outpoint(0)], 99_000);
    dust.output.push(pay(100));
    sign_key_path(&mut dust, 0, &prevouts, &key);
    assert!(fixture.check(&dust).unwrap_err().reason.starts_with("dust"));

    let mut overspend = spending(&[fixture.outpoint(0)], 100_001);
    sign_key_path(&mut overspend, 0, &prevouts, &key);
    assert_eq!(
        fixture.check(&overspend).unwrap_err().reason,
        "bad-txns-in-belowout"
    );

    let mut expensive = spending(&[fixture.outpoint(0)], 10_000);
    sign_key_path(&mut expensive, 0, &prevouts, &key);
    let context = Context {
        ledger: &fixture.ledger,
        mempool: &fixture.mempool,
        next_height: 2,
        tip_time: 1_700_000_000,
    };
    assert_eq!(
        check(&expensive, &context, Some(100)).unwrap_err().code,
        -25
    );
    check(&expensive, &context, None).unwrap();
}

#[test]
fn rejects_missing_duplicate_and_conflicting_inputs() {
    let key = secret(1);
    let mut fixture = Fixture::new(vec![funded(p2tr(&key), 100_000)]);
    let prevouts = [funded(p2tr(&key), 100_000)];

    let missing = spending(&[OutPoint::new(Txid::all_zeros(), 3)], 1_000);
    let rejection = fixture.check(&missing).unwrap_err();
    assert_eq!(
        (rejection.code, rejection.reason.as_str()),
        (-25, "bad-txns-inputs-missingorspent")
    );

    let duplicate = spending(&[fixture.outpoint(0), fixture.outpoint(0)], 1_000);
    assert_eq!(
        fixture.check(&duplicate).unwrap_err().reason,
        "bad-txns-inputs-duplicate"
    );

    let mut first = spending(&[fixture.outpoint(0)], 99_000);
    sign_key_path(&mut first, 0, &prevouts, &key);
    let accepted = fixture.check(&first).unwrap();
    fixture.mempool.add(Entry {
        txid: first.compute_txid(),
        tx: first.clone(),
        fee: accepted.fee,
        weight: accepted.weight,
    });
    assert_eq!(
        fixture.check(&first).unwrap_err().reason,
        "txn-already-in-mempool"
    );

    let mut second = spending(&[fixture.outpoint(0)], 98_000);
    sign_key_path(&mut second, 0, &prevouts, &key);
    assert_eq!(
        fixture.check(&second).unwrap_err().reason,
        "txn-mempool-conflict"
    );
}

#[test]
fn spends_an_unconfirmed_output_from_the_mempool() {
    let key = secret(1);
    let mut fixture = Fixture::new(vec![funded(p2tr(&key), 100_000)]);
    let prevouts = [funded(p2tr(&key), 100_000)];
    let change = TxOut {
        value: Amount::from_sat(99_000),
        script_pubkey: p2tr(&key),
    };
    let mut parent = spending(&[fixture.outpoint(0)], 0);
    parent.output = vec![change.clone()];
    sign_key_path(&mut parent, 0, &prevouts, &key);
    let accepted = fixture.check(&parent).unwrap();
    let parent_id = parent.compute_txid();
    fixture.mempool.add(Entry {
        txid: parent_id,
        tx: parent,
        fee: accepted.fee,
        weight: accepted.weight,
    });

    let mut child = spending(&[OutPoint::new(parent_id, 0)], 98_000);
    sign_key_path(&mut child, 0, &[change], &key);
    fixture.check(&child).unwrap();
}

#[test]
fn coinbase_outputs_mature_after_100_blocks() {
    let key = secret(1);
    let mut chain = Chain::regtest();
    let mut ledger = Ledger::default();
    ledger.apply_block(0, chain.block(0).unwrap());
    chain.mine(Vec::new(), p2tr(&key), Amount::ZERO, 0);
    ledger.apply_block(1, chain.block(1).unwrap());
    let coinbase = chain.block(1).unwrap().txdata[0].clone();
    let prevouts = [coinbase.output[0].clone()];

    let mut tx = spending(&[OutPoint::new(coinbase.compute_txid(), 0)], 4_999_000_000);
    sign_key_path(&mut tx, 0, &prevouts, &key);
    let mempool = Mempool::default();
    let at = |next_height| {
        check(
            &tx,
            &Context {
                ledger: &ledger,
                mempool: &mempool,
                next_height,
                tip_time: 1_700_000_000,
            },
            None,
        )
    };
    assert!(at(100)
        .unwrap_err()
        .reason
        .starts_with("bad-txns-premature-spend-of-coinbase"));
    at(101).unwrap();
}

#[test]
fn rejects_unsupported_scripts_by_name() {
    let fixture = Fixture::new(vec![funded(
        ScriptBuf::new_p2wsh(&ScriptBuf::from_bytes(vec![0x51]).wscript_hash()),
        10_000,
    )]);
    let tx = spending(&[fixture.outpoint(0)], 9_000);
    assert!(fixture
        .check(&tx)
        .unwrap_err()
        .reason
        .contains("pay-to-witness-script-hash"));
}

mod tapscript {
    use super::super::*;
    use bitcoin::script::Builder;

    fn run(script: ScriptBuf, stack: Vec<Vec<u8>>) -> Result<(), String> {
        let mut never = |_: &[u8], _: &[u8]| -> Result<bool, String> { Ok(true) };
        run_tapscript(&script, stack, &mut never)
    }

    fn script(build: impl FnOnce(Builder) -> Builder) -> ScriptBuf {
        build(Builder::new()).into_script()
    }

    #[test]
    fn skips_a_branch_that_is_not_taken() {
        let skipped = script(|b| {
            b.push_opcode(all::OP_PUSHBYTES_0)
                .push_opcode(all::OP_IF)
                .push_opcode(all::OP_RETURN)
                .push_opcode(all::OP_ENDIF)
                .push_int(1)
        });
        run(skipped, Vec::new()).unwrap();
    }

    #[test]
    fn requires_minimal_if_arguments_and_one_true_result() {
        let branch = script(|b| {
            b.push_opcode(all::OP_IF)
                .push_int(1)
                .push_opcode(all::OP_ENDIF)
        });
        assert!(run(branch.clone(), vec![vec![2]])
            .unwrap_err()
            .contains("minimal"));
        run(branch, vec![vec![1]]).unwrap();

        assert!(run(script(|b| b.push_int(1).push_int(1)), Vec::new())
            .unwrap_err()
            .contains("exactly one"));
        assert!(run(script(|b| b.push_int(0)), Vec::new())
            .unwrap_err()
            .contains("false"));
        assert!(run(script(|b| b.push_opcode(all::OP_IF)), vec![vec![1]])
            .unwrap_err()
            .contains("OP_IF"));
    }

    #[test]
    fn evaluates_hash_locks_and_arithmetic() {
        let preimage = b"secret".to_vec();
        let hash = sha256::Hash::hash(&preimage).to_byte_array();
        let lock = script(|b| {
            b.push_opcode(all::OP_SHA256)
                .push_slice(hash)
                .push_opcode(all::OP_EQUAL)
        });
        run(lock.clone(), vec![preimage]).unwrap();
        assert!(run(lock, vec![b"guess".to_vec()]).is_err());

        let sum = script(|b| {
            b.push_int(-3)
                .push_int(5)
                .push_opcode(all::OP_ADD)
                .push_int(2)
                .push_opcode(all::OP_NUMEQUAL)
        });
        run(sum, Vec::new()).unwrap();
    }

    #[test]
    fn counts_signatures_with_checksigadd() {
        let threshold = script(|b| {
            b.push_slice([1u8; 32])
                .push_opcode(all::OP_CHECKSIG)
                .push_slice([2u8; 32])
                .push_opcode(all::OP_CHECKSIGADD)
                .push_int(2)
                .push_opcode(all::OP_NUMEQUAL)
        });
        let mut only_first = |signature: &[u8], key: &[u8]| -> Result<bool, String> {
            Ok(!signature.is_empty() && key[0] == 1)
        };
        assert!(run_tapscript(&threshold, vec![Vec::new(), vec![9; 64]], &mut only_first).is_err());
        let mut both =
            |signature: &[u8], _: &[u8]| -> Result<bool, String> { Ok(!signature.is_empty()) };
        run_tapscript(&threshold, vec![vec![9; 64], vec![9; 64]], &mut both).unwrap();
    }

    #[test]
    fn op_success_succeeds_and_unknown_opcodes_are_named() {
        run(ScriptBuf::from_bytes(vec![0x50]), Vec::new()).unwrap();
        let separator = script(|b| b.push_opcode(all::OP_CODESEPARATOR).push_int(1));
        assert!(run(separator, Vec::new())
            .unwrap_err()
            .contains("OP_CODESEPARATOR"));
    }

    #[test]
    fn encodes_and_decodes_script_numbers() {
        for value in [
            0,
            1,
            -1,
            127,
            128,
            -128,
            255,
            256,
            -32_768,
            2_147_483_647,
            -2_147_483_647,
        ] {
            assert_eq!(number(&encode(value)).unwrap(), value, "{value}");
        }
        assert!(!truthy(&[0, 0, 0x80]));
        assert!(truthy(&[0, 1]));
    }
}
