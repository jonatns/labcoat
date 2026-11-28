//! The project's `.env` file: settings and the Subfrost API key kept beside
//! `labcoat.toml` instead of in the shell profile.
//!
//! The file is applied to the process environment once, before anything
//! reads a setting, so every reader sees the same values whether they came
//! from the shell or the file. That includes the pinned Alkanes client,
//! which reads `SUBFROST_API_KEY` itself. A variable the real environment
//! already sets is never replaced.
//!
//! Nothing here prints a value: reports carry names and line numbers only.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const FILE: &str = ".env";
pub const API_KEY: &str = "SUBFROST_API_KEY";
pub const PASSPHRASE: &str = "LABCOAT_WALLET_PASSPHRASE";
pub const MNEMONIC: &str = "LABCOAT_MNEMONIC";

/// The names `.env` may set. A file shared with other tools can hold
/// anything else; those lines are left alone.
const LOADED: &[&str] = &[
    API_KEY,
    "LABCOAT_NETWORK",
    "LABCOAT_RPC_URL",
    "LABCOAT_WALLET_FILE",
    "LABCOAT_FEE_RATE",
    "LABCOAT_SIGNER",
    "LABCOAT_ENVIRONMENT",
    "LABCOAT_PSBT_TIMEOUT_SECS",
    PASSPHRASE,
];

/// What a project's `.env` held and what was done with it. Names only.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    pub path: PathBuf,
    pub exists: bool,
    /// Why an existing file could not be read.
    pub unreadable: Option<String>,
    /// Set from the file.
    pub applied: Vec<String>,
    /// In the file, but the real environment already set them.
    pub shadowed: Vec<String>,
    /// `LABCOAT_*` names that `.env` does not set.
    pub ignored: Vec<String>,
    /// 1-based numbers of lines that are not `NAME=value`.
    pub malformed: Vec<usize>,
    /// The file holds a seed phrase, which is never read from it.
    pub mnemonic: bool,
}

impl Report {
    pub fn applied(&self, name: &str) -> bool {
        self.applied.iter().any(|applied| applied == name)
    }

    pub fn shadowed(&self, name: &str) -> bool {
        self.shadowed.iter().any(|shadowed| shadowed == name)
    }

    /// Problems worth a line on stderr on every command.
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let file = self.path.display();
        if let Some(error) = &self.unreadable {
            warnings.push(format!("{file} was not read: {error}"));
        }
        if !self.malformed.is_empty() {
            let lines: Vec<String> = self.malformed.iter().map(usize::to_string).collect();
            warnings.push(format!(
                "{file}: skipped line {} (expected NAME=value)",
                lines.join(", ")
            ));
        }
        if !self.ignored.is_empty() {
            warnings.push(format!(
                "{file}: {} cannot be set from .env and was ignored; set it in the environment",
                self.ignored.join(", ")
            ));
        }
        warnings
    }

    /// Why no command runs while the file holds a seed phrase.
    pub fn mnemonic_error(&self) -> Option<String> {
        self.mnemonic.then(|| {
            format!(
                "{} sets {MNEMONIC}; a seed phrase is not read from a plaintext file",
                self.path.display()
            )
        })
    }
}

pub const MNEMONIC_HINT: &str =
    "delete that line, then restore the wallet once with `labcoat wallet init --mnemonic-stdin`; the keystore keeps the seed encrypted";

/// Parse `.env` text into `(name, value)` pairs in file order, and the
/// numbers of the lines that are not assignments.
///
/// Accepted: `NAME=value`, an optional `export ` prefix, `#` comment lines,
/// a ` #` comment after an unquoted value, and single- or double-quoted
/// values (which may span lines; double quotes read `\n`, `\"`, `\\`).
fn parse(text: &str) -> (Vec<(String, String)>, Vec<usize>) {
    let mut pairs = Vec::new();
    let mut malformed = Vec::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines().enumerate();
    while let Some((index, raw)) = lines.next() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line
            .strip_prefix("export ")
            .map_or(line, |rest| rest.trim_start());
        let Some((name, value)) = line.split_once('=') else {
            malformed.push(number);
            continue;
        };
        let name = name.trim_end();
        let value = value.trim_start();
        let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''));
        let value = match quote {
            None => Some(unquoted(value)),
            Some(quote) => {
                // A quoted value runs to its closing quote, on this line or
                // a later one.
                let mut body = value[1..].to_string();
                loop {
                    if let Some(done) = quoted(&body, quote) {
                        break done;
                    }
                    match lines.next() {
                        Some((_, next)) => {
                            body.push('\n');
                            body.push_str(next);
                        }
                        None => break None,
                    }
                }
            }
        };
        match value {
            Some(value) if is_name(name) => pairs.push((name.to_string(), value)),
            _ => malformed.push(number),
        }
    }
    (pairs, malformed)
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unquoted(value: &str) -> String {
    let end = value
        .char_indices()
        .find(|&(at, c)| c == '#' && (at == 0 || value[..at].ends_with(char::is_whitespace)))
        .map_or(value.len(), |(at, _)| at);
    value[..end].trim_end().to_string()
}

/// The value inside `body` (the text after an opening `quote`), or `None`
/// while the closing quote has not been reached. Text after the closing
/// quote other than a comment makes the line malformed, reported as a
/// value that never closes.
fn quoted(body: &str, quote: char) -> Option<Option<String>> {
    let mut value = String::new();
    let mut chars = body.char_indices();
    while let Some((at, c)) = chars.next() {
        if c == quote {
            let rest = body[at + 1..].trim();
            return Some((rest.is_empty() || rest.starts_with('#')).then_some(value));
        }
        if c == '\\' && quote == '"' {
            match chars.next() {
                Some((_, 'n')) => value.push('\n'),
                Some((_, escaped @ ('"' | '\\'))) => value.push(escaped),
                Some((_, other)) => {
                    value.push('\\');
                    value.push(other);
                }
                None => value.push('\\'),
            }
        } else {
            value.push(c);
        }
    }
    None
}

/// Read `root/.env` and decide what it sets, given what the real
/// environment already holds. Returns the assignments to make; makes none.
pub fn plan(
    root: &Path,
    real: impl Fn(&str) -> Option<OsString>,
) -> (Report, Vec<(String, String)>) {
    let path = root.join(FILE);
    let mut report = Report {
        path: path.clone(),
        ..Report::default()
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (report, Vec::new()),
        Err(error) => {
            report.exists = true;
            report.unreadable = Some(error.to_string());
            return (report, Vec::new());
        }
    };
    report.exists = true;
    let (pairs, malformed) = parse(&text);
    report.malformed = malformed;
    let mut assignments: Vec<(String, String)> = Vec::new();
    for (name, value) in pairs {
        if name == MNEMONIC {
            report.mnemonic |= !value.is_empty();
        } else if LOADED.contains(&name.as_str()) {
            // The last assignment of a name is the one that counts.
            assignments.retain(|(earlier, _)| earlier != &name);
            assignments.push((name, value));
        } else if name.starts_with("LABCOAT_") && !report.ignored.contains(&name) {
            report.ignored.push(name);
        }
    }
    // An empty value is the placeholder `.env.example` ships.
    assignments.retain(|(_, value)| !value.is_empty());
    assignments.retain(|(name, _)| {
        // Set in the real environment, even to nothing, wins.
        let shadowed = real(name).is_some();
        if shadowed {
            report.shadowed.push(name.clone());
        } else {
            report.applied.push(name.clone());
        }
        !shadowed
    });
    (report, assignments)
}

static REPORT: OnceLock<Report> = OnceLock::new();
/// Every assignment in the file, for `${NAME}` in `labcoat.toml`. Kept
/// apart from the report so that nothing that prints a report holds a value.
static VALUES: OnceLock<Vec<(String, String)>> = OnceLock::new();

/// Every assignment in `root/.env` except a seed phrase, in file order.
fn file_values(root: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(root.join(FILE)) else {
        return Vec::new();
    };
    let (mut pairs, _) = parse(&text);
    pairs.retain(|(name, _)| name != MNEMONIC);
    pairs
}

/// The last assignment of `name` among `values`.
fn last_value(values: &[(String, String)], name: &str) -> Option<String> {
    values
        .iter()
        .rev()
        .find(|(assigned, _)| assigned == name)
        .map(|(_, value)| value.clone())
}

/// A variable as `labcoat.toml` sees it: the environment, then any
/// assignment in the project's `.env`, whatever its name. Names Labcoat
/// does not read itself stay out of the process environment and reach a
/// setting only where `labcoat.toml` asks for them.
pub fn var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| last_value(VALUES.get()?, name))
}

/// Apply the project's `.env` to this process's environment. Call once,
/// before any other thread exists: changing the environment is only safe
/// then.
pub fn apply(root: &Path) -> &'static Report {
    VALUES.get_or_init(|| file_values(root));
    REPORT.get_or_init(|| {
        let (report, assignments) = plan(root, |name| std::env::var_os(name));
        for (name, value) in assignments {
            std::env::set_var(name, value);
        }
        // The Alkanes client would send an empty key as a header. Set to
        // nothing means no key, which is also how one command opts out of
        // the key in the file.
        if std::env::var_os(API_KEY).is_some_and(|key| key.is_empty()) {
            std::env::remove_var(API_KEY);
        }
        report
    })
}

/// What `apply` found, when this command applied a `.env`.
pub fn report() -> Option<&'static Report> {
    REPORT.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str, env: Option<&str>) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("labcoat-dotenv-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        if let Some(env) = env {
            std::fs::write(root.join(FILE), env).unwrap();
        }
        root
    }

    fn pairs(text: &str) -> Vec<(String, String)> {
        parse(text).0
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_string(), value.to_string())
    }

    #[test]
    fn a_missing_file_sets_nothing_and_is_not_a_problem() {
        let root = project("missing", None);
        let (report, assignments) = plan(&root, |_| None);
        assert!(!report.exists);
        assert!(assignments.is_empty());
        assert!(report.warnings().is_empty());
        assert_eq!(report.mnemonic_error(), None);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_real_environment_wins_over_the_file() {
        let root = project(
            "precedence",
            Some("SUBFROST_API_KEY=from-file\nLABCOAT_NETWORK=signet\nLABCOAT_FEE_RATE=5\n"),
        );
        let (report, assignments) = plan(&root, |name| match name {
            "SUBFROST_API_KEY" => Some("from-shell".into()),
            // Set to nothing still counts: it is how one command opts out.
            "LABCOAT_FEE_RATE" => Some(OsString::new()),
            _ => None,
        });
        assert_eq!(assignments, [pair("LABCOAT_NETWORK", "signet")]);
        assert_eq!(report.applied, ["LABCOAT_NETWORK"]);
        assert_eq!(report.shadowed, ["SUBFROST_API_KEY", "LABCOAT_FEE_RATE"]);
        assert!(report.applied("LABCOAT_NETWORK") && report.shadowed(API_KEY));

        let (report, assignments) = plan(&root, |_| None);
        assert_eq!(assignments.len(), 3);
        assert!(report.applied(API_KEY) && report.shadowed.is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn only_labcoat_settings_and_the_api_key_are_loaded() {
        let root = project(
            "scope",
            Some(
                "DATABASE_URL=postgres://x\nPATH=/nowhere\nLABCOAT_DATA_HOME=/tmp/x\nLABCOAT_RPC_URL=http://mine\nSUBFROST_API_KEY=\n",
            ),
        );
        let (report, assignments) = plan(&root, |_| None);
        assert_eq!(assignments, [pair("LABCOAT_RPC_URL", "http://mine")]);
        // Other tools' variables are none of Labcoat's business; a LABCOAT_
        // name that is not loaded is worth saying.
        assert_eq!(report.ignored, ["LABCOAT_DATA_HOME"]);
        assert!(report.warnings()[0].contains("LABCOAT_DATA_HOME"));
        // The empty placeholder from .env.example is not a key.
        assert!(!report.applied(API_KEY));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_seed_phrase_in_the_file_is_refused_and_a_passphrase_is_loaded() {
        let root = project(
            "secrets",
            Some("LABCOAT_MNEMONIC=\"abandon abandon about\"\nLABCOAT_WALLET_PASSPHRASE=hunter2\n"),
        );
        let (report, assignments) = plan(&root, |_| None);
        assert!(report.mnemonic);
        assert_eq!(assignments, [pair(PASSPHRASE, "hunter2")]);
        let error = report.mnemonic_error().unwrap();
        assert!(error.contains("LABCOAT_MNEMONIC") && !error.contains("abandon"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn values_parse_like_a_shell_would_read_them() {
        assert_eq!(
            pairs(
                "\u{feff}# comment\n\nA=1\nexport B = two words  # trailing\n C='single # kept'\nD=\"a\\nb \\\"q\\\"\" # note\nE=\nF=has#hash\n"
            ),
            [
                pair("A", "1"),
                pair("B", "two words"),
                pair("C", "single # kept"),
                pair("D", "a\nb \"q\""),
                pair("E", ""),
                pair("F", "has#hash"),
            ]
        );
        // A quoted value may span lines, as other tools' PEM keys do.
        assert_eq!(
            parse("KEY=\"line one\nline two\"\nAFTER=1\n"),
            (
                vec![pair("KEY", "line one\nline two"), pair("AFTER", "1")],
                vec![]
            )
        );
    }

    #[test]
    fn malformed_lines_are_skipped_by_number_and_never_quoted() {
        let text = "SUBFROST_API_KEY=good\nsecret-without-equals\n1BAD=x\nBAD NAME=x\nQ=\"closed\" trailing\nOPEN='never closed\n";
        let (parsed, malformed) = parse(text);
        assert_eq!(parsed, [pair("SUBFROST_API_KEY", "good")]);
        assert_eq!(malformed, [2, 3, 4, 5, 6]);

        let root = project("malformed", Some(text));
        let (report, assignments) = plan(&root, |_| None);
        assert_eq!(assignments.len(), 1);
        let warnings = report.warnings().join("\n");
        assert!(warnings.contains("line 2, 3, 4, 5, 6"));
        for leaked in ["secret-without-equals", "good", "never closed"] {
            assert!(!warnings.contains(leaked));
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn labcoat_toml_can_name_any_variable_in_the_file_except_a_seed_phrase() {
        let root = project(
            "values",
            Some("MY_RPC=http://first\nMY_RPC=http://mine\nLABCOAT_MNEMONIC=abandon about\n"),
        );
        let values = file_values(&root);
        assert_eq!(
            last_value(&values, "MY_RPC").as_deref(),
            Some("http://mine")
        );
        assert_eq!(last_value(&values, MNEMONIC), None);
        assert_eq!(last_value(&values, "ABSENT"), None);
        // A name Labcoat does not read is not exported to the environment.
        let (report, assignments) = plan(&root, |_| None);
        assert!(assignments.is_empty() && report.ignored.is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_last_assignment_of_a_name_wins() {
        let root = project(
            "repeat",
            Some("LABCOAT_NETWORK=signet\nLABCOAT_NETWORK=mainnet\n"),
        );
        let (report, assignments) = plan(&root, |_| None);
        assert_eq!(assignments, [pair("LABCOAT_NETWORK", "mainnet")]);
        assert_eq!(report.applied, ["LABCOAT_NETWORK"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_report_never_holds_a_value() {
        let root = project(
            "no-values",
            Some("SUBFROST_API_KEY=sk-sentinel-value\nLABCOAT_WALLET_PASSPHRASE=pp-sentinel\nLABCOAT_MNEMONIC=mn-sentinel\n"),
        );
        let (report, _) = plan(&root, |_| None);
        let shown = format!(
            "{report:?} {:?} {:?}",
            report.warnings(),
            report.mnemonic_error()
        );
        assert!(!shown.contains("sentinel"));
        std::fs::remove_dir_all(root).ok();
    }
}
