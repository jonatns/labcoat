//! # labcoat-core
//!
//! Contract toolkit core on the pinned alkanes-rs main commit (see
//! TOOLCHAIN.md): wallet keystore, deploy (commit/reveal envelope),
//! execute, simulate, trace, UTXO queries, contract compilation, and the
//! labcoat.lock deployment ledger. The Rust `labcoat` CLI and MCP server
//! drive this crate.

pub mod abi;
pub mod apply;
pub mod compile;
pub mod error;
pub mod execute;
pub mod lockfile;
pub mod manifest;
pub mod simulate;
pub mod sync;
pub mod system;
pub mod toolkit;
pub mod trace;
pub mod wallet;
pub mod workspace;

pub use error::{LabcoatError, Result};
pub use system::{NetworkTarget, ToolkitConfig};

// Re-exports from the pinned alkanes-rs for downstream use.
pub use alkanes_cli_common::provider::ConcreteProvider;
pub use alkanes_support::cellpack::Cellpack;

/// Convert user-facing call arguments (decimal ints, 0x-hex, or UTF-8
/// strings) to u128 cellpack values. Strings become little-endian byte
/// integers.
pub fn parse_arg(arg: &str) -> Result<u128> {
    if let Some(hex_str) = arg.strip_prefix("0x") {
        return u128::from_str_radix(hex_str, 16).map_err(|e| {
            LabcoatError::new(
                "CONFIG_INVALID",
                format!("bad hex arg '{arg}': {e}"),
                "hex args must fit in u128",
            )
        });
    }
    if arg.chars().all(|c| c.is_ascii_digit()) && !arg.is_empty() {
        if let Ok(v) = arg.parse::<u128>() {
            return Ok(v);
        }
    }
    // UTF-8 string → little-endian u128 (matches TS encodeArg)
    let bytes = arg.as_bytes();
    if bytes.is_empty() || bytes.len() > 16 {
        return Err(LabcoatError::new(
            "CONFIG_INVALID",
            format!("string arg '{arg}' must be 1..=16 bytes"),
            "long strings don't fit a u128 cellpack value",
        ));
    }
    let mut value: u128 = 0;
    for (i, b) in bytes.iter().enumerate() {
        value |= (*b as u128) << (8 * i);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_hex_and_string_args() {
        assert_eq!(parse_arg("42").unwrap(), 42);
        assert_eq!(parse_arg("0xff").unwrap(), 255);
        // "AB" → 0x41 | 0x42<<8 (little-endian), same as TS encodeArg
        assert_eq!(parse_arg("AB").unwrap(), 0x4241);
        assert!(parse_arg("this-string-is-way-too-long!").is_err());
    }
}
