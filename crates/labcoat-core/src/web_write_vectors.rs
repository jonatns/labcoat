//! Test vectors that pin the generated web client's transaction builder to
//! the Rust call path.
//!
//! `labcoat generate web` writes a TypeScript client that builds contract
//! call transactions in the browser. It has to build what `labcoat call`
//! builds: the same inputs chosen from the same wallet, the same outputs,
//! protostone, fee, and change. Each case here runs the Rust path (the
//! options and spec `labcoat call` produces, Labcoat's protection of Alkane
//! carrying outputs, and the pinned upstream executor over a fixed wallet)
//! and records the unsigned transaction. The cases and results are written
//! to `tests/fixtures/generate-web/checks/write-vectors.json`, and the
//! TypeScript checks (`scripts/tests/generate-web-client-test.sh`) build the
//! same cases through the generated client and compare bytes.
//!
//! After an intentional change to either side:
//!
//! ```sh
//! LABCOAT_BLESS=1 cargo test -p labcoat-core web_write_vectors
//! ```

use crate::abi::{encode_method_args, AbiMethod, AbiParam};
use crate::execute::{
    execute_params, is_protected_carrier, layout_outputs, parse_spec, payment_outputs,
    verify_payments, ParamsInput, TxOptions,
};
use crate::toolkit::call_spec;
use alkanes_cli_common::alkanes::execute::EnhancedAlkanesExecutor;
use alkanes_cli_common::alkanes::types::{ExecutionState, InputRequirement};
use alkanes_cli_common::mock_provider::MockProvider;
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use bitcoin::{Address, Amount, Network, OutPoint, TxOut, Txid};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;

/// One wallet output: its outpoint index (the txid is derived from it), its
/// value, and the Alkanes the indexer reports on it.
struct Coin {
    sats: u64,
    alkanes: &'static [(u64, u64, u64)],
}

const fn coin(sats: u64) -> Coin {
    Coin { sats, alkanes: &[] }
}

struct Case {
    name: &'static str,
    coins: Vec<Coin>,
    contract: (u128, u128),
    method: AbiMethod,
    args: Vec<&'static str>,
    fee_rate: f32,
    /// `--inputs`, Alkanes only: `block:tx:amount` entries.
    alkanes_in: Vec<(u64, u64, u64)>,
    /// `--to`; `None` is the wallet's own address.
    to: Option<&'static str>,
    /// `--pay`, in order.
    payments: Vec<(&'static str, u64)>,
}

fn method(name: &str, opcode: u128, params: &[(&str, &str)]) -> AbiMethod {
    AbiMethod {
        name: name.to_string(),
        opcode,
        params: params
            .iter()
            .map(|(name, type_)| AbiParam {
                name: name.to_string(),
                type_: type_.to_string(),
            })
            .collect(),
        returns: "void".to_string(),
    }
}

/// Addresses of other wallets, one of each type a payment can go to.
const OTHER_P2TR: &str = "bcrt1p5cyxnuxmeuwuvkwfem96lqzszd02n6xdcjrs20cac6yqjjwudpxqvg32hk";
const OTHER_P2WPKH: &str = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080";
const OTHER_P2WSH: &str = "bcrt1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qzf4jry";
const OTHER_P2PKH: &str = "mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn";
const OTHER_P2SH: &str = "2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc";

fn cases() -> Vec<Case> {
    let plain = |name, coins: Vec<Coin>, fee_rate| Case {
        name,
        coins,
        contract: (2, 1),
        method: method("increment", 1, &[]),
        args: Vec::new(),
        fee_rate,
        alkanes_in: Vec::new(),
        to: None,
        payments: Vec::new(),
    };
    vec![
        plain("no arguments, one input, change", vec![coin(100_000)], 2.0),
        Case {
            contract: (4, 65_011),
            method: method(
                "configure",
                77,
                &[
                    ("amount", "u128"),
                    ("label", "String"),
                    ("asset", "AlkaneId"),
                ],
            ),
            args: vec!["340282366920938463463374607431768211455", "labcoat", "2:7"],
            ..plain(
                "u128, String, and AlkaneId arguments",
                vec![coin(250_000)],
                2.0,
            )
        },
        // Two cellpack words. The upstream mock wallet cannot answer the
        // inspection the executor runs after building a cellpack of 48
        // bytes or more, so no case here carries a longer one.
        Case {
            method: method("describe", 9, &[("text", "String")]),
            args: vec!["thirty characters of text here"],
            ..plain("a String argument of two words", vec![coin(50_000)], 3.0)
        },
        // 1,152 sats is the smallest input the selector accepts at
        // 1 sat/vB: it covers the 546 sat output and the fee, and leaves
        // less than a dust output of change, which goes to the fee.
        plain("change below dust is dropped", vec![coin(1_152)], 1.0),
        plain(
            "several small inputs",
            vec![coin(700), coin(900), coin(1_400), coin(3_000), coin(90_000)],
            2.0,
        ),
        plain(
            "a fee rate that is not a whole number",
            vec![coin(40_000)],
            1.1,
        ),
        plain("a high fee rate", vec![coin(600_000)], 87.5),
        Case {
            to: Some(OTHER_P2TR),
            ..plain("another recipient", vec![coin(75_000)], 2.0)
        },
        Case {
            payments: vec![(OTHER_P2WPKH, 10_000)],
            ..plain("a payment output", vec![coin(100_000)], 2.0)
        },
        Case {
            payments: vec![
                (OTHER_P2TR, 546),
                (OTHER_P2WSH, 25_000),
                (OTHER_P2PKH, 1_000),
                (OTHER_P2SH, 2_000),
            ],
            to: Some(OTHER_P2TR),
            ..plain(
                "payments to every address type",
                vec![coin(20_000), coin(30_000)],
                2.0,
            )
        },
        Case {
            method: method("deposit", 50, &[]),
            alkanes_in: vec![(2, 5, 300)],
            ..plain(
                "Alkanes sent in",
                vec![
                    Coin {
                        sats: 546,
                        alkanes: &[(2, 5, 1_000)],
                    },
                    coin(60_000),
                ],
                2.0,
            )
        },
        Case {
            method: method("swap", 51, &[("minimum", "u128")]),
            args: vec!["1"],
            alkanes_in: vec![(2, 5, 1_500), (4, 9, 2)],
            payments: vec![(OTHER_P2WPKH, 5_000)],
            ..plain(
                "Alkanes from several outputs, with a payment",
                vec![
                    coin(8_000),
                    Coin {
                        sats: 546,
                        alkanes: &[(2, 5, 1_000)],
                    },
                    // Holds an Alkane the call does not ask for: never spent.
                    Coin {
                        sats: 50_000,
                        alkanes: &[(2, 99, 7)],
                    },
                    Coin {
                        sats: 600,
                        alkanes: &[(2, 5, 900)],
                    },
                    Coin {
                        sats: 546,
                        alkanes: &[(4, 9, 2)],
                    },
                    // Not needed once the amounts above are collected.
                    Coin {
                        sats: 546,
                        alkanes: &[(2, 5, 50)],
                    },
                    coin(40_000),
                ],
                2.0,
            )
        },
        Case {
            ..plain(
                "outputs that carry Alkanes are not spent as bitcoin",
                vec![
                    Coin {
                        sats: 500_000,
                        alkanes: &[(2, 5, 1)],
                    },
                    coin(30_000),
                    Coin {
                        sats: 700_000,
                        alkanes: &[(2, 0, 10)],
                    },
                ],
                2.0,
            )
        },
    ]
}

fn txid(index: usize) -> Txid {
    Txid::from_str(&format!("{:064x}", 0xa0_0000 + index)).unwrap()
}

fn wallet() -> MockProvider {
    let secp = Secp256k1::new();
    let secret = SecretKey::from_slice(&[0x11; 32]).unwrap();
    let mut provider = MockProvider::new(Network::Regtest);
    provider.set_keypair(secret, bitcoin::PublicKey::new(secret.public_key(&secp)));
    provider
}

fn build(case: &Case) -> Value {
    let mut provider = wallet();
    let address = Address::p2tr(
        &provider.secp,
        provider.internal_key,
        None,
        Network::Regtest,
    );
    let outpoints: Vec<OutPoint> = (0..case.coins.len())
        .map(|index| OutPoint::new(txid(index), index as u32 % 3))
        .collect();
    for (coin, outpoint) in case.coins.iter().zip(&outpoints) {
        provider.utxos.lock().unwrap().push((
            *outpoint,
            TxOut {
                value: Amount::from_sat(coin.sats),
                script_pubkey: address.script_pubkey(),
            },
        ));
        if !coin.alkanes.is_empty() {
            provider
                .alkane_balances
                .lock()
                .unwrap()
                .insert(outpoint.to_string(), coin.alkanes.to_vec());
        }
    }

    // What `labcoat call` turns its flags into.
    let options = TxOptions {
        inputs: (!case.alkanes_in.is_empty()).then(|| {
            case.alkanes_in
                .iter()
                .map(|(block, tx, amount)| format!("{block}:{tx}:{amount}"))
                .collect::<Vec<_>>()
                .join(",")
        }),
        to: case.to.map(str::to_string),
        payments: case
            .payments
            .iter()
            .map(|(address, sats)| format!("{address}:{sats}"))
            .collect(),
        ..TxOptions::default()
    };
    let args: Vec<String> = case.args.iter().map(|arg| arg.to_string()).collect();
    let words = encode_method_args(&case.method, &args).unwrap();
    let spec = call_spec(
        case.contract.0,
        case.contract.1,
        case.method.opcode,
        &words,
        &options,
    );
    let payments = payment_outputs(&spec, &options, Network::Regtest).unwrap();
    let requirements = options.input_requirements().unwrap();
    let (to_addresses, input_requirements) = layout_outputs(
        options.to.clone().unwrap_or_else(|| address.to_string()),
        requirements.clone(),
        &payments,
    );

    // Labcoat's protection of Alkane carrying outputs.
    let required: BTreeSet<(u128, u128)> = requirements
        .iter()
        .filter_map(|requirement| match requirement {
            InputRequirement::Alkanes { block, tx, .. } => Some((*block as u128, *tx as u128)),
            _ => None,
        })
        .collect();
    let excluded_utxos = case
        .coins
        .iter()
        .zip(&outpoints)
        .filter(|(coin, _)| {
            !coin.alkanes.is_empty()
                && is_protected_carrier(
                    coin.alkanes.iter().map(|(block, tx, amount)| {
                        ((*block as u128, *tx as u128), *amount as u128)
                    }),
                    &required,
                )
        })
        .map(|(_, outpoint)| outpoint.to_string())
        .collect();

    let params = execute_params(ParamsInput {
        fee_rate: Some(case.fee_rate),
        to_addresses,
        input_requirements,
        protostones: parse_spec(&spec).unwrap(),
        envelope: None,
        excluded_utxos,
        max_indexed_height: None,
    });
    let state = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(EnhancedAlkanesExecutor::new(&mut provider).execute(params))
        .unwrap_or_else(|error| panic!("{}: {error}", case.name));
    let ExecutionState::ReadyToSign(ready) = state else {
        panic!("{}: a call is a single transaction", case.name);
    };
    let tx = &ready.psbt.unsigned_tx;
    verify_payments(tx, &payments).unwrap();

    json!({
        "name": case.name,
        "utxos": case.coins.iter().zip(&outpoints).map(|(coin, outpoint)| json!({
            "txid": outpoint.txid.to_string(),
            "vout": outpoint.vout,
            "value": coin.sats,
            "alkanes": coin.alkanes.iter().map(|(block, tx, amount)| json!({
                "id": format!("{block}:{tx}"),
                "amount": amount.to_string(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "contract": format!("{}:{}", case.contract.0, case.contract.1),
        "method": {
            "name": case.method.name,
            "opcode": case.method.opcode.to_string(),
            "params": case.method.params.iter().map(|param| json!({
                "name": param.name,
                "type": param.type_,
            })).collect::<Vec<_>>(),
            "returns": case.method.returns,
        },
        "args": case.args,
        "feeRate": case.fee_rate,
        "alkanes": case.alkanes_in.iter().map(|(block, tx, amount)| json!({
            "id": format!("{block}:{tx}"),
            "amount": amount.to_string(),
        })).collect::<Vec<_>>(),
        "to": case.to,
        "payments": case.payments.iter().map(|(address, sats)| json!({
            "address": address,
            "sats": sats,
        })).collect::<Vec<_>>(),
        "expected": {
            "spec": spec,
            "transaction": bitcoin::consensus::encode::serialize_hex(tx),
            "fee": ready.fee,
            "inputs": tx.input.iter().map(|input| input.previous_output.to_string()).collect::<Vec<_>>(),
            "outputs": tx.output.iter().map(|output| output.value.to_sat()).collect::<Vec<_>>(),
        },
    })
}

fn vectors_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/generate-web/checks/write-vectors.json")
}

#[test]
fn the_rust_call_path_builds_the_recorded_transactions() {
    let provider = wallet();
    let address = Address::p2tr(
        &provider.secp,
        provider.internal_key,
        None,
        Network::Regtest,
    );
    let cases: Vec<Value> = cases().iter().map(build).collect();

    // The cases are only worth their names if they build what they claim.
    let outputs = |name: &str| -> Vec<u64> {
        let case = cases.iter().find(|case| case["name"] == name).unwrap();
        serde_json::from_value(case["expected"]["outputs"].clone()).unwrap()
    };
    let inputs = |name: &str| -> usize {
        let case = cases.iter().find(|case| case["name"] == name).unwrap();
        case["expected"]["inputs"].as_array().unwrap().len()
    };
    assert_eq!(outputs("no arguments, one input, change").len(), 3);
    assert_eq!(outputs("change below dust is dropped"), vec![546, 0]);
    assert!(inputs("several small inputs") > 1);
    assert_eq!(outputs("a payment output")[1], 10_000);
    assert_eq!(inputs("Alkanes sent in"), 2);
    assert_eq!(
        inputs("outputs that carry Alkanes are not spent as bitcoin"),
        1
    );

    let document = json!({
        "comment": "Generated by crates/labcoat-core/src/web_write_vectors.rs; do not edit. Bless with LABCOAT_BLESS=1 cargo test -p labcoat-core web_write_vectors",
        "network": "regtest",
        "wallet": {
            "address": address.to_string(),
            "publicKey": provider.internal_key.to_string(),
        },
        "cases": cases,
    });
    let rendered = format!("{}\n", serde_json::to_string_pretty(&document).unwrap());
    let path = vectors_path();
    if std::env::var_os("LABCOAT_BLESS").is_some() {
        std::fs::write(&path, rendered).unwrap();
        return;
    }
    let recorded = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {} — run with LABCOAT_BLESS=1", path.display()));
    assert!(
        recorded == rendered,
        "the Rust call path no longer builds the transactions in {} — if intentional, bless with LABCOAT_BLESS=1 and make the generated web client match",
        path.display(),
    );
}
