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

    /// Best-effort classification of upstream (anyhow / AlkanesError)
    /// failures into stable codes.
    pub fn classify(err: anyhow::Error) -> Self {
        let msg = format!("{:#}", err);
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
                "is the devnet running? try `labcoat up` / `labcoat status`",
            )
        } else if lower.contains("insufficient") || lower.contains("not enough") {
            (
                "INSUFFICIENT_FUNDS",
                "fund the wallet: `labcoat fund <address>` then `labcoat mine 1`",
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
