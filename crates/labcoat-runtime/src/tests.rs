use super::*;
use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::key::{Keypair, TapTweak};
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::{transaction, Amount, OutPoint, Sequence, TxIn, TxOut, Witness};
use std::path::PathBuf;

/// Records the height of each block under `tip`; refuses block 300.
pub(crate) const RECORDER: &str = r#"
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
        (if (i32.eq (i32.load (i32.const 8192)) (i32.const 300)) (then unreachable))
        (i32.store (i32.const 2055) (i32.load (i32.const 8192)))
        (call $flush (i32.const 2048)))
      (func (export "tip") (result i32)
        (i32.store (i32.const 3996) (call $get_len (i32.const 3000)))
        (call $get (i32.const 3000) (i32.const 4000))
        (i32.const 4000)))
"#;

/// The same, with a different hash: an upgraded indexer.
const RECORDER_V2: &str = r#"
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
        (i32.store (i32.const 2055) (i32.load (i32.const 8192)))
        (call $flush (i32.const 2048)))
      (func (export "tip") (result i32)
        (i32.store (i32.const 3996) (call $get_len (i32.const 3000)))
        (call $get (i32.const 3000) (i32.const 4000))
        (i32.const 4000))
      (func (export "version") (result i32) (i32.const 0)))
"#;

pub(crate) fn module(source: &str) -> IndexerModule {
    IndexerModule::from_bytes(&wat::parse_str(source).unwrap()).unwrap()
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::regtest(module(RECORDER)).unwrap();
    runtime.set_clock(|| 0);
    runtime
}

fn tip(runtime: &Runtime) -> u32 {
    u32::from_le_bytes(runtime.view("tip", &[]).unwrap().try_into().unwrap())
}

fn key() -> SecretKey {
    SecretKey::from_slice(&[1; 32]).unwrap()
}

fn wallet_script() -> ScriptBuf {
    let secp = Secp256k1::new();
    ScriptBuf::new_p2tr(&secp, key().x_only_public_key(&secp).0, None)
}

fn anyone() -> ScriptBuf {
    ScriptBuf::from_bytes(vec![0x51])
}

/// A signed key-path spend of `coin` paying `value` back to the wallet.
fn spend(outpoint: OutPoint, coin: &TxOut, value: u64) -> Transaction {
    let secp = Secp256k1::new();
    let mut tx = Transaction {
        version: transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(value),
            script_pubkey: wallet_script(),
        }],
    };
    let hash = SighashCache::new(&tx)
        .taproot_key_spend_signature_hash(
            0,
            &Prevouts::All(std::slice::from_ref(coin)),
            TapSighashType::Default,
        )
        .unwrap();
    let keypair = Keypair::from_secret_key(&secp, &key()).tap_tweak(&secp, None);
    let signature = secp.sign_schnorr_no_aux_rand(
        &Message::from_digest(hash.to_byte_array()),
        &keypair.to_keypair(),
    );
    tx.input[0].witness = Witness::p2tr_key_spend(&bitcoin::taproot::Signature {
        signature,
        sighash_type: TapSighashType::Default,
    });
    tx
}

/// A runtime with a mature coinbase output paying the wallet.
fn funded() -> (Runtime, OutPoint, TxOut) {
    let mut runtime = runtime();
    runtime.mine(wallet_script()).unwrap();
    for _ in 0..100 {
        runtime.mine(anyone()).unwrap();
    }
    let coinbase = runtime.chain().block(1).unwrap().txdata[0].clone();
    (
        runtime,
        OutPoint::new(coinbase.compute_txid(), 0),
        coinbase.output[0].clone(),
    )
}

fn temporary(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("labcoat-runtime-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory.join("chain.log")
}

#[test]
fn indexes_genesis_and_every_mined_block() {
    let mut runtime = runtime();
    assert_eq!((runtime.height(), tip(&runtime)), (0, 0));
    assert_eq!(runtime.mine(anyone()).unwrap(), 1);
    assert_eq!(runtime.mine(anyone()).unwrap(), 2);
    assert_eq!((runtime.height(), tip(&runtime)), (2, 2));
}

#[test]
fn mines_accepted_transactions_and_pays_their_fees() {
    let (mut runtime, outpoint, coin) = funded();
    let tx = spend(outpoint, &coin, coin.value.to_sat() - 2_000);
    let txid = runtime.submit(tx.clone(), None).unwrap();
    assert_eq!(runtime.mempool().len(), 1);
    assert_eq!(
        runtime.submit(tx, None).unwrap_err().reason,
        "txn-already-in-mempool"
    );

    let height = runtime.mine(anyone()).unwrap();
    let block = runtime.chain().block(height).unwrap();
    assert_eq!(block.txdata[1].compute_txid(), txid);
    assert_eq!(
        block.txdata[0].output[0].value,
        chain::subsidy(height) + Amount::from_sat(2_000)
    );
    assert!(runtime.mempool().is_empty());
    assert!(runtime.ledger().coin(&outpoint).is_none());
    assert!(runtime.ledger().coin(&OutPoint::new(txid, 0)).is_some());
}

#[test]
fn keeps_the_mempool_when_the_indexer_rejects_a_block() {
    let (mut runtime, outpoint, coin) = funded();
    while runtime.height() < 299 {
        runtime.mine(anyone()).unwrap();
    }
    runtime
        .submit(spend(outpoint, &coin, coin.value.to_sat() - 2_000), None)
        .unwrap();

    let error = runtime.mine(anyone()).unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::Indexer(IndexerError::Trap { .. })
    ));
    assert_eq!((runtime.height(), tip(&runtime)), (299, 299));
    assert_eq!(
        runtime.mempool().len(),
        1,
        "the transaction waits for the next block"
    );
    assert!(runtime.ledger().coin(&outpoint).is_some());
}

#[test]
fn reverts_to_a_snapshot() {
    let (mut runtime, outpoint, coin) = funded();
    let snapshot = runtime.snapshot();
    let height = runtime.height();
    runtime
        .submit(spend(outpoint, &coin, coin.value.to_sat() - 2_000), None)
        .unwrap();
    runtime.mine(anyone()).unwrap();
    assert!(runtime.ledger().coin(&outpoint).is_none());

    runtime.revert(snapshot.clone()).unwrap();
    assert_eq!((runtime.height(), tip(&runtime)), (height, height));
    assert!(runtime.ledger().coin(&outpoint).is_some());
    assert!(runtime.mempool().is_empty());

    runtime.mine(anyone()).unwrap();
    let later = runtime.snapshot();
    runtime.revert(snapshot).unwrap();
    assert!(matches!(
        runtime.revert(later),
        Err(RuntimeError::SnapshotAhead { .. })
    ));
}

#[test]
fn refuses_a_snapshot_from_a_branch_that_was_reverted_away() {
    let mut runtime = runtime();
    let start = runtime.snapshot();
    runtime.mine(anyone()).unwrap();
    runtime.mine(anyone()).unwrap();
    let discarded = runtime.snapshot();

    runtime.revert(start).unwrap();
    runtime.set_clock(|| 1_000_000_000);
    runtime.mine(wallet_script()).unwrap();
    runtime.mine(wallet_script()).unwrap();
    let height = runtime.height();
    let storage = runtime.indexer().storage().clone();
    assert_eq!(
        height,
        discarded.height(),
        "back at the discarded snapshot's height"
    );

    assert!(matches!(
        runtime.revert(discarded),
        Err(RuntimeError::SnapshotOffChain { snapshot: 2 })
    ));
    assert_eq!(runtime.height(), height);
    assert_eq!(runtime.indexer().storage(), &storage);
}

#[test]
fn reopens_a_saved_chain_without_indexing_again() {
    let path = temporary("reopen");
    let mut runtime = Runtime::open(module(RECORDER), &path).unwrap();
    runtime.mine(wallet_script()).unwrap();
    runtime.mine(anyone()).unwrap();
    let hash = runtime.chain().tip().block_hash();
    let storage = runtime.indexer().storage().clone();
    drop(runtime);

    let reopened = Runtime::open(module(RECORDER), &path).unwrap();
    assert_eq!(reopened.height(), 2);
    assert_eq!(reopened.chain().tip().block_hash(), hash);
    assert_eq!(reopened.indexer().storage(), &storage);
    assert_eq!(tip(&reopened), 2);
    assert_eq!(reopened.ledger().coin_count(), 2, "both coinbase outputs");
}

#[test]
fn indexes_a_saved_chain_again_for_a_different_module() {
    let path = temporary("upgrade");
    let mut runtime = Runtime::open(module(RECORDER), &path).unwrap();
    runtime.mine(anyone()).unwrap();
    runtime.mine(anyone()).unwrap();
    drop(runtime);

    let upgraded = Runtime::open(module(RECORDER_V2), &path).unwrap();
    assert_eq!((upgraded.height(), tip(&upgraded)), (2, 2));
    drop(upgraded);
    match store::BlockLog::open(&path, &module(RECORDER_V2).hash()).unwrap() {
        store::Opened::Matching(log, _) => assert_eq!(log.len(), 3),
        _ => panic!("the chain file should now name the new module"),
    }
}

#[test]
fn a_block_that_cannot_be_saved_is_not_mined() {
    let path = temporary("unsaved");
    let mut runtime = Runtime::open(module(RECORDER), &path).unwrap();
    runtime.set_clock(|| 0);
    runtime.mine(wallet_script()).unwrap();
    for _ in 0..100 {
        runtime.mine(anyone()).unwrap();
    }
    let coinbase = runtime.chain().block(1).unwrap().txdata[0].clone();
    let outpoint = OutPoint::new(coinbase.compute_txid(), 0);
    runtime
        .submit(
            spend(
                outpoint,
                &coinbase.output[0],
                coinbase.output[0].value.to_sat() - 2_000,
            ),
            None,
        )
        .unwrap();
    let height = runtime.height();
    let storage = runtime.indexer().storage().clone();

    let saved = std::fs::metadata(&path).unwrap().len();
    runtime.log.as_mut().unwrap().fail_writes_for_test(true);
    assert!(matches!(
        runtime.mine(anyone()),
        Err(RuntimeError::Store(_))
    ));
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        saved,
        "the partial record is removed"
    );
    assert_eq!(runtime.height(), height);
    assert_eq!(runtime.indexer().tip(), Some(height));
    assert_eq!(runtime.indexer().storage(), &storage);
    assert_eq!(
        runtime.mempool().len(),
        1,
        "the transaction waits for the next block"
    );
    assert!(runtime.ledger().coin(&outpoint).is_some());

    // Once the disk accepts writes again, mining continues where it was,
    // and the saved chain reopens.
    runtime.log.as_mut().unwrap().fail_writes_for_test(false);
    assert_eq!(runtime.mine(anyone()).unwrap(), height + 1);
    assert!(runtime.ledger().coin(&outpoint).is_none());
    let tip = runtime.chain().tip().block_hash();
    drop(runtime);
    let reopened = Runtime::open(module(RECORDER), &path).unwrap();
    assert_eq!(reopened.chain().tip().block_hash(), tip);
}

#[test]
fn a_revert_is_saved() {
    let path = temporary("revert");
    let mut runtime = Runtime::open(module(RECORDER), &path).unwrap();
    runtime.mine(anyone()).unwrap();
    let snapshot = runtime.snapshot();
    runtime.mine(anyone()).unwrap();
    runtime.mine(anyone()).unwrap();
    runtime.revert(snapshot).unwrap();
    runtime.mine(anyone()).unwrap();
    let hash = runtime.chain().tip().block_hash();
    drop(runtime);

    let reopened = Runtime::open(module(RECORDER), &path).unwrap();
    assert_eq!(reopened.height(), 2);
    assert_eq!(reopened.chain().tip().block_hash(), hash);
}

mod rpc {
    use super::*;
    use crate::node::Node;
    use serde_json::{json, Value};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;

    fn call(node: &Node, method: &str, params: Value) -> Value {
        let response = node.handle_request(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
        );
        assert!(response.get("error").is_none(), "{method}: {response}");
        response["result"].clone()
    }

    fn error(node: &Node, method: &str, params: Value) -> (i64, String) {
        let response = node.handle_request(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
        );
        let error = &response["error"];
        (
            error["code"].as_i64().unwrap(),
            error["message"].as_str().unwrap().to_owned(),
        )
    }

    fn address() -> String {
        bitcoin::Address::from_script(&wallet_script(), bitcoin::Network::Regtest)
            .unwrap()
            .to_string()
    }

    #[test]
    fn mines_sends_and_looks_up_like_a_bitcoin_node() {
        let node = Node::new(runtime());
        let hashes = call(&node, "generatetoaddress", json!([101, address()]));
        assert_eq!(hashes.as_array().unwrap().len(), 101);
        assert_eq!(call(&node, "getblockcount", json!([])), json!(101));

        let first = call(&node, "getblock", json!([hashes[0], 1]));
        let coinbase = first["tx"][0].as_str().unwrap().to_owned();
        let out = call(&node, "gettxout", json!([coinbase, 0, true]));
        assert_eq!(out["confirmations"], json!(101));
        assert_eq!(out["coinbase"], json!(true));

        let coin = TxOut {
            value: Amount::from_sat(5_000_000_000),
            script_pubkey: wallet_script(),
        };
        let outpoint = OutPoint::new(coinbase.parse().unwrap(), 0);
        let tx = spend(outpoint, &coin, 4_999_990_000);
        let raw = bitcoin::consensus::encode::serialize_hex(&tx);
        let txid = call(&node, "sendrawtransaction", json!([raw]));
        assert_eq!(txid, json!(tx.compute_txid().to_string()));
        assert_eq!(call(&node, "getmempoolinfo", json!([]))["size"], json!(1));
        assert_eq!(
            call(&node, "gettxout", json!([coinbase, 0, true])),
            Value::Null
        );
        assert_eq!(
            error(&node, "sendrawtransaction", json!([raw])).1,
            "txn-already-in-mempool"
        );

        let mined = call(&node, "generatetoaddress", json!([1, address()]));
        let block = call(&node, "getblock", json!([mined[0], 2]));
        assert_eq!(block["tx"][1]["txid"], txid);
        let fetched = call(&node, "getrawtransaction", json!([txid, true]));
        assert_eq!(fetched["confirmations"], json!(1));
        assert_eq!(call(&node, "getrawtransaction", json!([txid])), json!(raw));
    }

    #[test]
    fn answers_esplora_views_in_esplorashrew_form() {
        let node = Node::new(runtime());
        call(&node, "generatetoaddress", json!([2, address()]));
        let hashed = {
            let mut hash = crate::ledger::script_hash(&wallet_script());
            hash.reverse();
            bitcoin::hex::DisplayHex::to_lower_hex_string(&hash[..])
        };
        let view = |name: &str, input: &str| -> String {
            let result = call(
                &node,
                "secondaryview",
                json!([
                    "esplora",
                    name,
                    bitcoin::hex::DisplayHex::to_lower_hex_string(input.as_bytes())
                ]),
            );
            let hex = result.as_str().unwrap().strip_prefix("0x").unwrap();
            String::from_utf8(<Vec<u8> as bitcoin::hex::FromHex>::from_hex(hex).unwrap()).unwrap()
        };

        let utxos: Value = serde_json::from_str(&view("utxosbyscripthash", &hashed)).unwrap();
        assert_eq!(utxos.as_array().unwrap().len(), 2);
        assert_eq!(utxos[0]["status"]["confirmed"], json!(true));
        assert_eq!(utxos[0]["status"]["block_height"], json!(1));
        assert_eq!(utxos[0]["value"], json!(5_000_000_000u64));

        let txid = utxos[0]["txid"].as_str().unwrap();
        let tx: Value = serde_json::from_str(&view("tx", txid)).unwrap();
        assert_eq!(tx["vin"][0]["is_coinbase"], json!(true));
        assert_eq!(tx["vout"][0]["scriptpubkey_type"], json!("v1_p2tr"));
        assert!(view("txhex", txid).starts_with("02000000"));
        assert_eq!(view("tx", &"00".repeat(32)), r#"{"error":"tx not found"}"#);
        assert_eq!(view("tipheight", ""), "2");
        assert_eq!(call(&node, "secondaryheight", json!(["esplora"])), json!(2));
        assert_eq!(
            error(
                &node,
                "secondaryview",
                json!(["brc20-prog", "getutxo", "00"])
            )
            .1,
            "indexer 'brc20-prog' not found"
        );
    }

    #[test]
    fn reports_rejections_with_bitcoin_core_codes() {
        let node = Node::new(runtime());
        assert_eq!(error(&node, "sendrawtransaction", json!(["00"])).0, -22);
        assert_eq!(
            error(&node, "getblockhash", json!([5])),
            (-8, "Block height out of range".into())
        );
        assert_eq!(
            error(&node, "generatetoaddress", json!([1, "not-an-address"])).0,
            -5
        );
        assert_eq!(
            error(&node, "nosuchmethod", json!([])),
            (-32601, "Method not found: nosuchmethod".into())
        );
    }

    #[test]
    fn says_who_is_serving() {
        let node = Node::new(runtime()).with_data_dir("/data/network");
        let info = call(&node, "labcoat_info", json!([]));
        assert_eq!(info["name"], json!("Labcoat Network"));
        assert_eq!(info["version"], json!(env!("CARGO_PKG_VERSION")));
        assert_eq!(info["dataDir"], json!("/data/network"));
    }

    #[test]
    fn snapshots_and_reverts() {
        let node = Node::new(runtime());
        let id = call(&node, "labcoat_snapshot", json!([]));
        call(&node, "generatetoaddress", json!([3, address()]));
        assert_eq!(call(&node, "labcoat_revert", json!([id])), json!(true));
        assert_eq!(call(&node, "getblockcount", json!([])), json!(0));
        assert_eq!(
            call(&node, "labcoat_revert", json!([id])),
            json!(false),
            "a snapshot is used once"
        );
    }

    #[test]
    fn serves_requests_and_batches_over_http_until_stopped() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let node = Arc::new(Node::new(runtime()));
        let server = std::thread::spawn({
            let node = node.clone();
            move || crate::server::serve(node, listener)
        });

        let mut stream = TcpStream::connect(address).unwrap();
        let mut exchange = |body: &str| -> Value {
            write!(stream, "POST / HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut response = Vec::new();
            let mut byte = [0u8; 1];
            while !response.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                response.push(byte[0]);
            }
            let head = String::from_utf8(response).unwrap();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse()
                .unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            serde_json::from_slice(&body).unwrap()
        };

        // Two requests on one kept-alive connection, then a batch.
        assert_eq!(
            exchange(r#"{"jsonrpc":"2.0","id":7,"method":"getblockcount","params":[]}"#)["id"],
            json!(7)
        );
        let batch =
            exchange(r#"[{"id":1,"method":"getblockcount"},{"id":2,"method":"getbestblockhash"}]"#);
        assert_eq!(batch.as_array().unwrap().len(), 2);
        assert_eq!(
            exchange(r#"{"id":3,"method":"stop"}"#)["result"],
            json!("Labcoat Network stopping")
        );
        server.join().unwrap().unwrap();
        assert!(node.stopping());
    }
}
