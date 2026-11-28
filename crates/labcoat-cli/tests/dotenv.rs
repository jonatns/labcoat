//! The project's `.env`, exercised through the built binary: what it sets,
//! what wins over it, and that no secret in it reaches any output.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const KEY: &str = "sk-sentinel-0123456789";
const PASSPHRASE: &str = "pp-sentinel-passphrase";

fn project(name: &str, env: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("labcoat-dotenv-it-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(root.join("contracts/counter")).unwrap();
    std::fs::write(root.join("labcoat.toml"), "network = \"labcoat\"\n").unwrap();
    std::fs::write(root.join(".env"), env).unwrap();
    root
}

/// Run `labcoat` in `dir` with none of the developer's own settings.
fn labcoat(dir: &Path, root: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_labcoat"));
    command.current_dir(dir).args(args);
    for (name, _) in std::env::vars() {
        if name.starts_with("LABCOAT_") || name == "SUBFROST_API_KEY" || name == "RUST_LOG" {
            command.env_remove(name);
        }
    }
    command
        .env("LABCOAT_DATA_HOME", root.join("data-home"))
        .env("NO_COLOR", "1")
        .envs(env.iter().copied());
    command.output().expect("labcoat runs")
}

fn text(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn check<'a>(envelope: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    envelope["result"]["checks"]
        .as_array()
        .expect("doctor checks")
        .iter()
        .find(|check| check["name"] == name)
        .unwrap_or_else(|| panic!("doctor has no `{name}` check"))
}

fn doctor(dir: &Path, root: &Path, env: &[(&str, &str)]) -> serde_json::Value {
    let output = labcoat(dir, root, &["--json", "doctor"], env);
    serde_json::from_slice(&output.stdout).expect("doctor prints one JSON envelope")
}

#[test]
fn settings_come_from_the_file_unless_the_environment_sets_them() {
    let root = project(
        "precedence",
        &format!("SUBFROST_API_KEY={KEY}\nLABCOAT_NETWORK=signet\n"),
    );
    // Found from a subdirectory: the file belongs to the project root.
    let envelope = doctor(&root.join("contracts/counter"), &root, &[]);
    assert!(check(&envelope, "project config")["detail"]
        .as_str()
        .unwrap()
        .contains("targets signet at https://signet.subfrost.io"));
    assert_eq!(
        check(&envelope, "Subfrost API key")["detail"],
        "SUBFROST_API_KEY is set (from .env)"
    );
    assert_eq!(
        check(&envelope, ".env")["detail"]
            .as_str()
            .unwrap()
            .split(" sets ")
            .nth(1),
        Some("SUBFROST_API_KEY, LABCOAT_NETWORK")
    );

    let envelope = doctor(
        &root,
        &root,
        &[
            ("LABCOAT_NETWORK", "mainnet"),
            ("SUBFROST_API_KEY", "other"),
        ],
    );
    assert!(check(&envelope, "project config")["detail"]
        .as_str()
        .unwrap()
        .contains("targets mainnet"));
    assert_eq!(
        check(&envelope, "Subfrost API key")["detail"],
        "SUBFROST_API_KEY is set (from the environment, over the one in .env)"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_project_without_the_file_reports_no_key() {
    let root = project("missing", "");
    std::fs::remove_file(root.join(".env")).unwrap();
    let envelope = doctor(&root, &root, &[("LABCOAT_NETWORK", "signet")]);
    let key = check(&envelope, "Subfrost API key");
    assert_eq!(key["status"], "warn");
    assert!(key["hint"].as_str().unwrap().contains(".env"));
    assert!(envelope["result"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["name"] != ".env"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn secrets_in_the_file_never_reach_any_output() {
    let root = project(
        "secrets",
        &format!(
            "SUBFROST_API_KEY={KEY}\nLABCOAT_WALLET_PASSPHRASE={PASSPHRASE}\nLABCOAT_NETWORK=signet\nnot an assignment {KEY}\n"
        ),
    );
    let wallet = root.join("no-such-wallet.json");
    let wallet = wallet.to_str().unwrap();
    let runs: &[&[&str]] = &[
        &["doctor"],
        &["--json", "doctor"],
        &["--verbose", "doctor"],
        // A command that resolves settings, builds the client the key is
        // handed to, and fails before any request.
        &["--verbose", "--wallet-file", wallet, "wallet", "addresses"],
        &["--json", "--wallet-file", wallet, "wallet", "addresses"],
        &["--verbose", "--wallet-file", wallet, "wallet", "utxos"],
        &["--json", "lock", "show"],
        &["docs"],
    ];
    for args in runs {
        let output = labcoat(&root, &root, args, &[("RUST_LOG", "trace")]);
        let shown = text(&output);
        assert!(!shown.is_empty(), "{args:?} printed nothing");
        for secret in [KEY, PASSPHRASE] {
            assert!(!shown.contains(secret), "{args:?} printed a secret");
        }
    }
    // The malformed line is reported by number alone.
    let output = labcoat(
        &root,
        &root,
        &["--wallet-file", wallet, "wallet", "addresses"],
        &[],
    );
    assert!(text(&output).contains("skipped line 4"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_seed_phrase_in_the_file_stops_the_command_without_echoing_it() {
    let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let root = project("mnemonic", &format!("LABCOAT_MNEMONIC=\"{phrase}\"\n"));
    let output = labcoat(&root, &root, &["--json", "wallet", "init"], &[]);
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "CONFIG_INVALID");
    assert!(envelope["error"]["hint"]
        .as_str()
        .unwrap()
        .contains("--mnemonic-stdin"));
    assert!(!text(&output).contains("abandon"));
    // No wallet was created in place of the one the developer meant to restore.
    assert!(!root.join("data-home").join("wallets").exists());

    let envelope = doctor(&root, &root, &[]);
    assert_eq!(check(&envelope, ".env")["status"], "fail");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_mainnet_passphrase_from_the_file_is_called_out() {
    let root = project(
        "mainnet",
        &format!("LABCOAT_WALLET_PASSPHRASE={PASSPHRASE}\n"),
    );
    let wallet = root.join("no-such-wallet.json");
    let run = |network: &str| {
        text(&labcoat(
            &root,
            &root,
            &[
                "--network",
                network,
                "--wallet-file",
                wallet.to_str().unwrap(),
                "wallet",
                "addresses",
            ],
            &[],
        ))
    };
    let mainnet = run("mainnet");
    assert!(mainnet.contains("LABCOAT_WALLET_PASSPHRASE was read from .env on mainnet"));
    assert!(!mainnet.contains(PASSPHRASE));
    assert!(!run("signet").contains("read from .env"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn labcoat_toml_reads_a_variable_the_file_defines() {
    let root = project("interpolate", "MY_RPC=http://127.0.0.1:19999/custom\n");
    std::fs::write(
        root.join("labcoat.toml"),
        "network = \"regtest\"\nrpc_url = \"${MY_RPC}\"\n",
    )
    .unwrap();
    let envelope = doctor(&root, &root, &[]);
    assert!(check(&envelope, "project config")["detail"]
        .as_str()
        .unwrap()
        .contains("targets regtest at http://127.0.0.1:19999/custom"));
    // The environment still wins over the file.
    let envelope = doctor(&root, &root, &[("MY_RPC", "http://127.0.0.1:19998/shell")]);
    assert!(check(&envelope, "project config")["detail"]
        .as_str()
        .unwrap()
        .contains("19998/shell"));

    std::fs::write(root.join(".env"), "").unwrap();
    let envelope = doctor(&root, &root, &[]);
    let config = check(&envelope, "project config");
    assert_eq!(config["status"], "fail");
    assert!(config["detail"].as_str().unwrap().contains("${MY_RPC}"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn a_token_labcoat_toml_puts_in_the_endpoint_is_shown_nowhere() {
    const TOKEN: &str = "tok-sentinel-0123456789";
    let root = project("redact", &format!("RPC_TOKEN={TOKEN}\n"));
    // Port 1 refuses at once, so the RPC client reports the URL it tried.
    std::fs::write(
        root.join("labcoat.toml"),
        "network = \"regtest\"\nrpc_url = \"http://127.0.0.1:1/${RPC_TOKEN}/rpc\"\n",
    )
    .unwrap();
    let runs: &[&[&str]] = &[
        &["doctor"],
        &["--json", "doctor"],
        &["simulate", "2:0", "99"],
        &["--json", "simulate", "2:0", "99"],
        &["--verbose", "simulate", "2:0", "99"],
        &[
            "--json",
            "balance",
            "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080",
        ],
        &["generate", "web"],
        &["--json", "generate", "web"],
    ];
    for args in runs {
        let shown = text(&labcoat(&root, &root, args, &[("RUST_LOG", "trace")]));
        assert!(!shown.contains(TOKEN), "{args:?} printed the token");
    }
    // The host is still named, in the error the RPC client wrote.
    let failed = text(&labcoat(&root, &root, &["simulate", "2:0", "99"], &[]));
    assert!(
        failed.contains("http://127.0.0.1:1/[redacted]/rpc"),
        "{failed}"
    );

    // A browser downloads the manifest: it names the endpoint without the token.
    let generated = text(&labcoat(&root, &root, &["generate", "web"], &[]));
    assert!(generated.contains("the manifest records the endpoint without"));
    let manifest = std::fs::read_to_string(root.join("generated/web/manifest.ts")).unwrap();
    assert!(manifest.contains("rpcUrl: \"http://127.0.0.1:1/[redacted]/rpc\""));
    for entry in std::fs::read_dir(root.join("generated/web")).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            assert!(!std::fs::read_to_string(&path).unwrap().contains(TOKEN));
        }
    }
    std::fs::remove_dir_all(root).ok();
}
