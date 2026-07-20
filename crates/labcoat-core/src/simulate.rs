//! Read-only contract simulation via the metashrew simulate view.
//!
//! Mirrors alkanes-cli's `alkanes simulate` construction; result decoding
//! reproduces the old TS `decodeAlkanesResult` (printable-string first,
//! then integer).

use crate::error::{LabcoatError, Result};
use alkanes_cli_common::proto::alkanes::{MessageContextParcel, SimulateResponse};
use alkanes_cli_common::provider::ConcreteProvider;
use alkanes_cli_common::traits::{AlkanesProvider, MetashrewRpcProvider};
use prost::Message;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateOutcome {
    pub status: String,
    pub gas_used: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Raw return data as 0x-hex.
    pub data: String,
    /// Convenience decodings of `data`.
    pub decoded: DecodedData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodedData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string: Option<String>,
    /// Little-endian integer value of the data (decimal string), when it fits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uint: Option<String>,
}

pub async fn simulate(
    provider: &ConcreteProvider,
    block: u128,
    tx: u128,
    opcode: u128,
    args: &[u128],
) -> Result<SimulateOutcome> {
    let mut inputs = vec![opcode];
    inputs.extend_from_slice(args);

    let cellpack = alkanes_support::cellpack::Cellpack {
        target: alkanes_support::id::AlkaneId { block, tx },
        inputs,
    };
    let calldata = cellpack.encipher();

    let height = provider
        .get_metashrew_height()
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;

    let context = MessageContextParcel {
        alkanes: Vec::new(),
        transaction: Vec::new(),
        block: Vec::new(),
        height,
        vout: 0,
        txindex: 0,
        calldata,
        pointer: 0,
        refund_pointer: 0,
    };

    let contract_id = format!("{}:{}", block, tx);
    let result = AlkanesProvider::simulate(provider, &contract_id, &context, None)
        .await
        .map_err(|e| LabcoatError::classify(e.into()))?;

    decode_result(&result)
}

fn decode_result(result: &serde_json::Value) -> Result<SimulateOutcome> {
    // The provider returns the raw view response — usually a hex string of
    // a protobuf SimulateResponse.
    let hex_str = result.as_str().ok_or_else(|| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("unexpected simulate response shape: {}", result),
            "re-run with RUST_LOG=debug",
        )
    })?;
    let bytes = hex::decode(hex_str.strip_prefix("0x").unwrap_or(hex_str)).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("simulate response is not hex: {}", e),
            "re-run with RUST_LOG=debug",
        )
    })?;
    let response = SimulateResponse::decode(bytes.as_slice()).map_err(|e| {
        LabcoatError::new(
            "TOOLKIT_ERROR",
            format!("failed to decode SimulateResponse: {}", e),
            "the pinned alkanes-rs rev and the devnet indexer may be out of sync",
        )
    })?;

    let data = response
        .execution
        .as_ref()
        .map(|e| e.data.clone())
        .unwrap_or_default();

    let status = if response.error.is_empty() {
        "success".to_string()
    } else {
        "revert".to_string()
    };

    Ok(SimulateOutcome {
        status,
        gas_used: response.gas_used,
        error: if response.error.is_empty() {
            None
        } else {
            Some(response.error.clone())
        },
        data: format!("0x{}", hex::encode(&data)),
        decoded: decode_data(&data),
    })
}

/// Printable-ASCII string first, then the little-endian integer convention
/// used by Alkanes contract responses.
fn decode_data(data: &[u8]) -> DecodedData {
    let string = std::str::from_utf8(data).ok().and_then(|s| {
        let printable = !s.is_empty()
            && s.chars()
                .all(|c| (' '..='~').contains(&c) || c.is_whitespace());
        if printable {
            Some(s.to_string())
        } else {
            None
        }
    });

    let uint = if !data.is_empty() && data.len() <= 16 {
        let mut padded = [0_u8; 16];
        padded[..data.len()].copy_from_slice(data);
        Some(u128::from_le_bytes(padded).to_string())
    } else {
        None
    };

    DecodedData { string, uint }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_printable_string() {
        let d = decode_data(b"MyToken");
        assert_eq!(d.string.as_deref(), Some("MyToken"));
    }

    #[test]
    fn decodes_integer() {
        let d = decode_data(&[0x01, 0x00]);
        assert_eq!(d.uint.as_deref(), Some("1"));
        assert!(d.string.is_none());
    }

    #[test]
    fn decodes_counter_response_as_little_endian_u128() {
        let d = decode_data(&hex::decode("03000000000000000000000000000000").unwrap());
        assert_eq!(d.uint.as_deref(), Some("3"));
        assert!(d.string.is_none());
    }

    #[test]
    fn empty_data_decodes_to_nothing() {
        let d = decode_data(&[]);
        assert!(d.string.is_none());
        assert!(d.uint.is_none());
    }
}
