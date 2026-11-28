//! What a state-changing command is doing while a public network makes it
//! wait.
//!
//! Labcoat Network mines a block for every transaction, so a deploy or a
//! call there finishes in about a second and has nothing to report. On
//! signet, testnet, and mainnet the next block is minutes away: the
//! transaction id is worth showing the moment it is broadcast, and the wait
//! for a block should say that it is a wait. The toolkit reports those
//! moments here; whoever drives it (the CLI) decides how to show them.
//!
//! Nothing is reported on regtest targets.

use std::sync::OnceLock;

/// One moment of a broadcast or of the wait that follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    /// The network accepted a transaction. `kind` is `transaction` for a
    /// call or a clone, `commit` and `reveal` for the two halves of a
    /// contract deploy.
    Broadcast { kind: &'static str, txid: &'a str },
    /// The transaction is sent; waiting for a block to confirm it and for
    /// the Alkanes index to trace it.
    Waiting { txid: &'a str },
    /// The wait is over: `confirmed` when the trace arrived, otherwise the
    /// time allowed ran out and the transaction is still unconfirmed.
    WaitEnded { txid: &'a str, confirmed: bool },
}

type Reporter = Box<dyn Fn(Event<'_>) + Send + Sync>;

static REPORTER: OnceLock<Reporter> = OnceLock::new();

/// Receive progress events for the rest of the process. The first caller
/// wins; without one, events are dropped.
pub fn set_reporter(reporter: impl Fn(Event<'_>) + Send + Sync + 'static) {
    let _ = REPORTER.set(Box::new(reporter));
}

pub(crate) fn report(config: &crate::ToolkitConfig, event: Event<'_>) {
    if config.network.uses_regtest() {
        return;
    }
    if let Some(reporter) = REPORTER.get() {
        reporter(event);
    }
}
