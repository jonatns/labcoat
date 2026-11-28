//! What `labcoat deploy` and `labcoat call` show while a public network
//! makes them wait.
//!
//! A transaction sent to signet, testnet, or mainnet confirms with the next
//! block, about ten minutes away. Its id is printed the moment it is
//! broadcast, the wait says that it is a wait, and Ctrl-C during the wait
//! ends the command without pretending anything was cancelled: the
//! transaction is already sent.
//!
//! Everything here goes to stderr; stdout stays the command's result.

use crate::output::{self, ColorMode};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use labcoat_core::progress::Event;
use std::io::IsTerminal;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

/// Exit code of a deploy or a call whose transaction was sent and had not
/// confirmed when the command stopped waiting. Not 0, so `deploy && next`
/// does not run `next` against a contract that does not exist yet; not 1,
/// because nothing failed and running the command again carries on.
pub const EXIT_SENT_UNCONFIRMED: i32 = 3;

/// Which command is waiting, for the line that says what to run next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Call,
    /// `labcoat deploy` of deploy.rs steps: running it again resumes.
    DeploySteps,
    /// `labcoat deploy --wasm`: one transaction outside deploy.rs.
    DeployWasm,
}

struct Wait {
    txid: String,
    started: Instant,
    bar: Option<ProgressBar>,
    stop_waiting: tokio::sync::oneshot::Sender<()>,
}

#[derive(Default)]
struct State {
    wait: Option<Wait>,
}

/// Show broadcasts and waits for the rest of this process.
pub fn install(command: Command, network: &str, color: ColorMode, json: bool) {
    let state = Arc::new(Mutex::new(State::default()));
    let network = network.to_string();
    let interrupts = Once::new();
    labcoat_core::progress::set_reporter(move |event| match event {
        Event::Broadcast { kind, txid } => {
            output::print_progress(&broadcast_line(kind, txid), color);
        }
        Event::Waiting { txid, stop_waiting } => {
            let bar = spinner(&network, json);
            if bar.is_none() {
                output::print_progress(&waiting_line(&network), color);
            }
            state.lock().expect("wait state").wait = Some(Wait {
                txid: txid.to_string(),
                started: Instant::now(),
                bar,
                stop_waiting,
            });
            interrupts.call_once(|| watch_interrupts(state.clone(), command, color));
        }
        Event::WaitEnded { txid, confirmed } => {
            let wait = state.lock().expect("wait state").wait.take();
            let elapsed = wait.as_ref().map(|wait| wait.started.elapsed());
            if let Some(bar) = wait.and_then(|wait| wait.bar) {
                bar.finish_and_clear();
            }
            if confirmed {
                output::print_progress(
                    &format!(
                        "confirmed {txid}{}",
                        elapsed
                            .map(|elapsed| format!(" after {}", human(elapsed)))
                            .unwrap_or_default()
                    ),
                    color,
                );
            }
        }
    });
}

fn broadcast_line(kind: &str, txid: &str) -> String {
    match kind {
        "commit" => format!("sent commit transaction {txid}"),
        "reveal" => format!("sent reveal transaction {txid}"),
        _ => format!("sent transaction {txid}"),
    }
}

/// The one line a log file or a pipe gets for the whole wait.
fn waiting_line(network: &str) -> String {
    format!(
        "waiting for the next {network} block to confirm it (blocks arrive about every 10 minutes); Ctrl-C stops waiting, the transaction stays sent"
    )
}

/// A live line with the time waited so far, when someone is watching.
fn spinner(network: &str, json: bool) -> Option<ProgressBar> {
    if json || !std::io::stderr().is_terminal() {
        return None;
    }
    let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
    bar.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg} {elapsed_precise:.dim}")
            .expect("static progress template")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    bar.set_message(format!(
        "Waiting for the next {network} block (about every 10 minutes; Ctrl-C stops waiting)…"
    ));
    bar.enable_steady_tick(Duration::from_millis(90));
    Some(bar)
}

fn human(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

/// What Ctrl-C during the wait prints. Nothing is undone, so it says so
/// and names the command that picks the result up later.
fn interrupted_message(command: Command, txid: &str) -> (String, String) {
    let headline =
        format!("Stopped waiting. Transaction {txid} is already sent and can still confirm.");
    let next = match command {
        Command::DeploySteps => {
            "Run `labcoat deploy` again to wait for it and run the remaining steps.".to_string()
        }
        Command::Call | Command::DeployWasm => {
            format!("Run `labcoat trace {txid} --wait` to see its result.")
        }
    };
    (headline, next)
}

/// Ctrl-C during a confirmation wait returns control to the command so it
/// can finish reporting the pending transaction. An interrupt outside a
/// wait keeps the usual exit status.
fn watch_interrupts(state: Arc<Mutex<State>>, command: Command, color: ColorMode) {
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_err() {
            return;
        }
        let wait = state.lock().expect("wait state").wait.take();
        let Some(wait) = wait else {
            std::process::exit(130);
        };
        if let Some(bar) = &wait.bar {
            bar.finish_and_clear();
        }
        let (headline, next) = interrupted_message(command, &wait.txid);
        output::print_warning(&headline, color);
        output::print_hint(&next, color);
        let _ = wait.stop_waiting.send(());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broadcast_names_which_transaction_was_sent() {
        assert_eq!(broadcast_line("transaction", "ab"), "sent transaction ab");
        assert_eq!(broadcast_line("commit", "ab"), "sent commit transaction ab");
        assert_eq!(broadcast_line("reveal", "cd"), "sent reveal transaction cd");
    }

    #[test]
    fn the_waiting_line_says_what_is_awaited_and_that_ctrl_c_is_safe() {
        let line = waiting_line("signet");
        assert!(line.contains("next signet block"));
        assert!(line.contains("the transaction stays sent"));
    }

    #[test]
    fn stopping_the_wait_says_the_transaction_is_sent_and_what_to_run() {
        let (headline, next) = interrupted_message(Command::Call, "ab");
        assert!(headline.contains("Transaction ab is already sent"));
        assert!(headline.contains("can still confirm"));
        assert_eq!(next, "Run `labcoat trace ab --wait` to see its result.");

        let (_, next) = interrupted_message(Command::DeploySteps, "ab");
        assert!(next.contains("`labcoat deploy` again"));
        let (_, next) = interrupted_message(Command::DeployWasm, "ab");
        assert!(next.contains("`labcoat trace ab --wait`"));
    }

    #[test]
    fn elapsed_time_reads_in_minutes_once_it_is_minutes() {
        assert_eq!(human(Duration::from_secs(42)), "42s");
        assert_eq!(human(Duration::from_secs(9 * 60 + 5)), "9m 05s");
    }
}
