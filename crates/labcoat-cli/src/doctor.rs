//! `labcoat doctor` — environment diagnosis: toolchain, ports, disk,
//! binaries, project state. Read-only; every check reports pass/warn/fail
//! with a fix-it hint.

use isomer_core::{BinaryManager, IsomerConfig, ProcessManager, ServiceId};
use serde::Serialize;
use std::path::{Path, PathBuf};

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

/// The nearest ancestor of `cwd` that holds `labcoat.toml`.
fn project_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|dir| dir.join("labcoat.toml").is_file())
        .map(Path::to_path_buf)
}

/// The `[toolchain] channel` of a `rust-toolchain.toml`, if the file exists
/// and names one.
fn toolchain_channel(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    value
        .get("toolchain")?
        .get("channel")?
        .as_str()
        .map(str::to_owned)
}

/// Whether rustup already has `channel`; `None` when rustup cannot answer.
/// Listing toolchains never installs one, unlike running cargo through a
/// pinned project.
fn rustup_toolchain_installed(channel: &str) -> Option<bool> {
    let output = std::process::Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let prefix = format!("{channel}-");
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .any(|name| name == channel || name.starts_with(&prefix)),
    )
}

/// Whether the project's alkanes-rs, metashrew, and labcoat-test pins match
/// the installed CLI. The toolchain pin has its own check.
fn project_pins_check(root: &Path) -> Check {
    const NAME: &str = "project pins";
    match crate::upgrade::plan(root) {
        Ok(report) if report.managed == 0 => warn(
            NAME,
            "Cargo.toml has no alkanes-rs or labcoat-test git dependencies",
            "compare Cargo.toml with a project from `labcoat init`",
        ),
        Ok(report) => {
            let stale: Vec<String> = report
                .changes
                .iter()
                .filter(|change| change.file == "Cargo.toml")
                .map(|change| {
                    format!(
                        "{} ({} → {})",
                        change.item,
                        change.from.as_deref().unwrap_or("unpinned"),
                        change.to
                    )
                })
                .collect();
            if stale.is_empty() {
                ok(
                    NAME,
                    format!(
                        "alkanes-rs and labcoat-test match Labcoat {}",
                        env!("CARGO_PKG_VERSION")
                    ),
                )
            } else {
                warn(
                    NAME,
                    format!(
                        "pinned for a different Labcoat release: {}",
                        stale.join(", ")
                    ),
                    "run `labcoat upgrade` (preview with --dry-run), then `labcoat test`",
                )
            }
        }
        Err(error) => warn(NAME, error.message, error.hint),
    }
}

fn project_toolchain_check(pinned: Option<&str>, expected: &str, installed: Option<bool>) -> Check {
    const NAME: &str = "project toolchain";
    match pinned {
        None => warn(
            NAME,
            "no rust-toolchain.toml; contracts build with your default Rust toolchain",
            format!("run `labcoat upgrade` to pin Rust {expected} and the WebAssembly targets"),
        ),
        Some(channel) if channel != expected => warn(
            NAME,
            format!("project pins Rust {channel}; this Labcoat release is tested with {expected}"),
            format!("set channel = \"{expected}\" in rust-toolchain.toml, then run `labcoat test`"),
        ),
        Some(channel) => ok(
            NAME,
            match installed {
                Some(true) => format!("Rust {channel} (installed)"),
                Some(false) => format!("Rust {channel} (rustup installs it on the first build)"),
                None => format!("Rust {channel}"),
            },
        ),
    }
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
    let rustup = version_of("rustup", "--version");
    match &rustup {
        Some(v) => checks.push(ok("rustup", v.clone())),
        None => checks.push(warn(
            "rustup",
            "not found on PATH",
            "install rustup from https://rustup.rs; only rustup applies a project's rust-toolchain.toml",
        )),
    }
    let project = project_root(&std::env::current_dir().unwrap_or_default());
    let pinned = project
        .as_deref()
        .map(|root| toolchain_channel(&root.join("rust-toolchain.toml")));
    let mut wasm_target_pending = false;
    if let Some(pinned) = pinned {
        let installed = match (&pinned, &rustup) {
            (Some(channel), Some(_)) => rustup_toolchain_installed(channel),
            _ => None,
        };
        wasm_target_pending = installed == Some(false);
        checks.push(project_toolchain_check(
            pinned.as_deref(),
            labcoat_core::compile::PROJECT_RUST_TOOLCHAIN,
            installed,
        ));
    }
    if let Some(root) = project.as_deref() {
        checks.push(project_pins_check(root));
    }
    if wasm_target_pending {
        checks.push(ok(
            "wasm32-unknown-unknown",
            "installed with the project toolchain on the first build",
        ));
    } else {
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
        checks.push(ok("ports", "all Labcoat Network ports are free"));
    } else {
        // Occupied ports are fine when they belong to Labcoat Network — report as warn.
        checks.push(warn(
            "ports",
            format!("in use: {}", busy.join(", ")),
            "if this isn't Labcoat Network, stop the other process or change the Labcoat Network ports",
        ));
    }

    // Binaries
    let infos = BinaryManager::new().check_all();
    let problems: Vec<String> = infos
        .iter()
        .filter_map(|binary| match &binary.status {
            isomer_core::BinaryStatus::Installed { .. } => None,
            isomer_core::BinaryStatus::NotInstalled => {
                Some(format!("{} is not installed", binary.service))
            }
            isomer_core::BinaryStatus::Invalid { reason } => {
                Some(format!("{} is invalid: {reason}", binary.service))
            }
            isomer_core::BinaryStatus::Unsupported { platform } => {
                Some(format!("{} is unsupported on {platform}", binary.service))
            }
        })
        .collect();
    if problems.is_empty() {
        checks.push(ok(
            "runtime bundle",
            format!("{} is installed", BinaryManager::release_tag()),
        ));
    } else {
        checks.push(fail(
            "runtime bundle",
            problems.join("; "),
            "run `labcoat binaries --download`; unsupported platforms require a supported machine",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_toolchain_check_reports_missing_matching_and_stale_pins() {
        let missing = project_toolchain_check(None, "1.100.0", None);
        assert_eq!(missing.status, "warn");
        assert!(missing.hint.unwrap().contains("1.100.0"));

        let stale = project_toolchain_check(Some("1.88.0"), "1.100.0", Some(true));
        assert_eq!(stale.status, "warn");
        assert!(stale.detail.contains("1.88.0"));
        assert!(stale.hint.unwrap().contains("channel = \"1.100.0\""));

        let pending = project_toolchain_check(Some("1.100.0"), "1.100.0", Some(false));
        assert_eq!(pending.status, "ok");
        assert!(pending.detail.contains("first build"));
    }

    #[test]
    fn toolchain_channel_reads_the_pinned_channel() {
        let dir = std::env::temp_dir().join(format!("labcoat-doctor-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("labcoat.toml"), "").unwrap();
        let file = dir.join("rust-toolchain.toml");
        std::fs::write(&file, "[toolchain]\nchannel = \"1.100.0\"\n").unwrap();
        assert_eq!(toolchain_channel(&file).as_deref(), Some("1.100.0"));
        assert_eq!(
            project_root(&dir.join("nested")).as_deref(),
            Some(dir.as_path())
        );
        assert_eq!(toolchain_channel(&dir.join("missing.toml")), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
