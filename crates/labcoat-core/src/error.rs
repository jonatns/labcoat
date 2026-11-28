//! Error codes with agent-friendly hints.
//!
//! Every failure that crosses the CLI/TS boundary carries a stable `code`
//! and a `hint` naming the next thing to try.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LabcoatError {
    pub code: &'static str,
    pub message: String,
    pub hint: &'static str,
}

impl std::fmt::Display for LabcoatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for LabcoatError {}

impl LabcoatError {
    pub fn new(code: &'static str, message: impl Into<String>, hint: &'static str) -> Self {
        Self {
            code,
            message: message.into(),
            hint,
        }
    }

    /// Why a `cargo` command could not be started. Cargo missing from
    /// `PATH` is its own error: every other message would send the
    /// developer looking at their project instead of their toolchain.
    pub fn cargo_unavailable(error: &std::io::Error, action: &str) -> Self {
        if error.kind() == std::io::ErrorKind::NotFound {
            Self::new(
                "TOOLCHAIN_MISSING",
                format!("cannot {action}: `cargo` was not found on PATH"),
                "install Rust with rustup (https://rustup.rs), open a new shell, then run `labcoat doctor`",
            )
        } else {
            Self::new(
                "TOOLKIT_ERROR",
                format!("cannot {action}: failed to run cargo: {error}"),
                "run `labcoat doctor`",
            )
        }
    }

    /// Best-effort classification of upstream (anyhow / AlkanesError)
    /// failures into stable codes.
    pub fn classify(err: anyhow::Error) -> Self {
        let msg = format!("{err:#}");
        let lower = msg.to_lowercase();
        let (code, hint) = if lower.contains("wallet file does not exist")
            || lower.contains("no wallet")
            || lower.contains("no keystore")
        {
            ("WALLET_MISSING", "run `labcoat wallet init` first")
        } else if lower.contains("decrypt") || lower.contains("passphrase") {
            (
                "WALLET_LOCKED",
                "set LABCOAT_WALLET_PASSPHRASE to the wallet passphrase",
            )
        } else if lower.contains("connection refused")
            || lower.contains("error sending request")
            || lower.contains("connect")
        {
            (
                "RPC_UNREACHABLE",
                "is Labcoat Network running? try `labcoat up` / `labcoat status`",
            )
        } else if lower.contains("excluded because their alkane balance could not be") {
            // The wallet's outputs are there, but upstream asks the indexer
            // what each one carries before spending it and leaves out any
            // whose answer failed. It then reports the shortfall as
            // insufficient funds, which sends the developer to fund a
            // wallet that is already funded. A rate-limited endpoint is the
            // usual cause: one request per output, in a burst.
            (
                "RPC_REJECTED",
                "the wallet was not short of funds: the endpoint did not answer the Alkanes balance check for some of its outputs, so they were left out. Subfrost's public endpoints allow 20 requests a minute and answer the rest with HTTP 429, so set SUBFROST_API_KEY (https://api.subfrost.io), or wait a minute and retry; nothing was signed or broadcast",
            )
        } else if lower.contains("unknown rpc error") {
            // Upstream drops a response body that is not JSON-RPC, which is
            // what a hosted endpoint's rate limiter sends.
            (
                "RPC_REJECTED",
                "the endpoint refused the request; Subfrost's public endpoints allow 20 requests a minute, so set SUBFROST_API_KEY (https://api.subfrost.io) or wait and retry",
            )
        } else if lower.contains("insufficient") || lower.contains("not enough") {
            (
                "INSUFFICIENT_FUNDS",
                "fund the wallet and wait for a confirmation; on Labcoat Network run `labcoat up`, or `labcoat fund <address>` then `labcoat mine 1`",
            )
        } else if lower.contains("revert") {
            (
                "EXECUTION_REVERT",
                "inspect the decoded revert reason in the message",
            )
        } else {
            ("TOOLKIT_ERROR", "re-run with RUST_LOG=debug for details")
        };
        Self::new(code, msg, hint)
    }
}

pub type Result<T> = std::result::Result<T, LabcoatError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The pinned alkanes-cli-common's messages, word for word
    /// (`alkanes/execute.rs`, UTXO selection).
    const UNKNOWN_BALANCES: &str = "Wallet error: Insufficient verified bitcoin: need 75546 sats, have 0 from UTXOs with a verified alkane balance; 3 UTXO(s) were excluded because their alkane balance could not be determined (indexer probe failed or returned malformed data). Spending them in an alkanes transaction could forward unrequested tokens to the called contract; retry once the indexer is reachable";
    const UNKNOWN_ALKANES: &str = "Wallet error: Insufficient alkanes: need 5 of 2:1, have 0 (2 UTXO(s) were excluded because their alkane balance could not be verified; retry once the indexer is reachable)";

    #[test]
    fn outputs_left_out_for_an_unanswered_balance_check_are_an_endpoint_problem() {
        for message in [UNKNOWN_BALANCES, UNKNOWN_ALKANES] {
            let error = LabcoatError::classify(anyhow::anyhow!(message));
            assert_eq!(error.code, "RPC_REJECTED", "{message}");
            assert!(error.hint.contains("SUBFROST_API_KEY"));
            assert!(error.hint.contains("not short of funds"));
            // Nothing in it sends the developer to fund a funded wallet.
            assert!(!error.hint.contains("labcoat fund"));
        }
    }

    #[test]
    fn a_wallet_that_is_short_is_still_insufficient_funds() {
        let error = LabcoatError::classify(anyhow::anyhow!(
            "Wallet error: Insufficient funds: need 75546 sats, have 1200"
        ));
        assert_eq!(error.code, "INSUFFICIENT_FUNDS");
        assert!(error.hint.contains("fund the wallet"));
    }

    #[test]
    fn a_rate_limited_response_is_a_rejected_request() {
        // What upstream makes of Subfrost's HTTP 429 body, whose `error`
        // is a string rather than a JSON-RPC error object.
        let error = LabcoatError::classify(anyhow::anyhow!("RPC error: Code 0: Unknown RPC error"));
        assert_eq!(error.code, "RPC_REJECTED");
        assert!(error.hint.contains("SUBFROST_API_KEY"));
    }
}
