//! `labcoat doctor` — environment diagnosis: toolchain, ports, disk,
//! binaries, project state. Read-only; every check reports pass/warn/fail
//! with a fix-it hint.

use isomer_core::{BinaryManager, IsomerConfig, ProcessManager, ServiceId};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub status: &'static str, // "ok" | "warn" | "fail"
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

fn ok(name: &str, detail: impl Into<String>) -> Check {
    Check {
        name: name.to_string(),
        status: "ok",
        detail: detail.into(),
        hint: None,
    }
}

fn warn(name: &str, detail: impl Into<String>, hint: impl Into<String>) -> Check {
    Check {
        name: name.to_string(),
        status: "warn",
        detail: detail.into(),
        hint: Some(hint.into()),
    }
}

fn fail(name: &str, detail: impl Into<String>, hint: impl Into<String>) -> Check {
    Check {
        name: name.to_string(),
        status: "fail",
        detail: detail.into(),
        hint: Some(hint.into()),
    }
}

fn version_of(cmd: &str, arg: &str) -> Option<String> {
    std::process::Command::new(cmd)
        .arg(arg)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
}

pub async fn run() -> Vec<Check> {
    let mut checks = Vec::new();

    // Toolchain
    match version_of("cargo", "--version") {
        Some(v) => checks.push(ok("cargo", v)),
        None => checks.push(fail(
            "cargo",
            "not found on PATH",
            "install Rust via rustup (contract compilation needs cargo)",
        )),
    }
    let wasm_target = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-unknown-unknown"))
        .unwrap_or(false);
    if wasm_target {
        checks.push(ok("wasm32-unknown-unknown", "target installed"));
    } else {
        checks.push(warn(
            "wasm32-unknown-unknown",
            "target not reported by rustup",
            "rustup target add wasm32-unknown-unknown",
        ));
    }
    match labcoat_core::compile::wasm_c_compiler() {
        Some(path) => checks.push(ok(
            "wasm C compiler",
            format!("{} supports wasm32", path.display()),
        )),
        None => checks.push(fail(
            "wasm C compiler",
            "no LLVM clang with a wasm32 backend found",
            "install LLVM (`brew install llvm` on macOS, `apt install clang wasi-libc` on Linux)",
        )),
    }
    match version_of("node", "--version") {
        Some(v) => checks.push(ok("node", v)),
        None => checks.push(fail(
            "node",
            "not found on PATH",
            "install Node.js 20+ (the devnet JSON-RPC gateway runs on node)",
        )),
    }

    // Ports
    let config = IsomerConfig::load();
    let mut busy = Vec::new();
    for service in ServiceId::all() {
        let port = ProcessManager::port_for_service(service, &config);
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
            busy.push(format!("{} :{}", service.id(), port));
        }
    }
    if busy.is_empty() {
        checks.push(ok("ports", "all devnet ports are free"));
    } else {
        // Occupied ports are fine when it's our own devnet — report as warn.
        checks.push(warn(
            "ports",
            format!("in use: {}", busy.join(", ")),
            "if this isn't a running labcoat devnet, stop the other process or change the devnet ports",
        ));
    }

    // Binaries
    let infos = BinaryManager::new().check_all();
    let missing: Vec<String> = infos
        .iter()
        .filter(|b| matches!(b.status, isomer_core::BinaryStatus::NotInstalled))
        .map(|b| b.service.clone())
        .collect();
    if missing.is_empty() {
        checks.push(ok("service binaries", "all installed"));
    } else {
        checks.push(warn(
            "service binaries",
            format!("missing: {}", missing.join(", ")),
            "labcoat binaries --download (or labcoat up)",
        ));
    }

    // Disk space in the data dir
    let data_dir = isomer_core::get_data_dir();
    let probe = data_dir.join(".doctor-probe");
    let disk_writable =
        std::fs::create_dir_all(&data_dir).is_ok() && std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    if disk_writable {
        checks.push(ok(
            "data dir",
            format!("{} is writable", data_dir.display()),
        ));
    } else {
        checks.push(fail(
            "data dir",
            format!("{} is not writable", data_dir.display()),
            "check permissions/disk space",
        ));
    }

    // Project state
    if std::path::Path::new("labcoat.lock").exists() {
        checks.push(ok("labcoat.lock", "present"));
    } else if std::path::Path::new("deployments/manifest.json").exists() {
        checks.push(warn(
            "labcoat.lock",
            "legacy deployments/manifest.json found without a lockfile",
            "labcoat lock migrate",
        ));
    }
    if std::path::Path::new(".labcoat/wallet.json").exists() {
        checks.push(ok("wallet", ".labcoat/wallet.json present"));
    } else {
        checks.push(warn(
            "wallet",
            "no project wallet keystore",
            "labcoat wallet init",
        ));
    }

    checks
}
