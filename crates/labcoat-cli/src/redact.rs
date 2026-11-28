//! Keeps the parts of `rpc_url` that `labcoat.toml` took from a variable
//! out of everything Labcoat prints or writes for others to read.
//!
//! `rpc_url = "https://node.example/${TOKEN}/jsonrpc"` is how a hosted
//! endpoint's key gets into a URL. The URL is needed whole to make
//! requests, and nowhere else: the target line, `doctor`, results, error
//! text from the RPC client, logs, MCP answers, and the generated web
//! manifest all show `[redacted]` in the variable's place. The scheme and
//! host stay visible, so the line that says where a write is headed still
//! says it.

use std::sync::Mutex;

pub const MASK: &str = "[redacted]";

/// A part shorter than this is not hidden: it is a port or a path segment
/// such as `v1`, and masking every `v1` in the output would garble it.
const SHORTEST: usize = 8;

static SECRETS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The parts of `expanded` to hide, given the byte ranges that came from
/// variables: what those ranges cover outside the scheme and host.
pub fn secret_parts(expanded: &str, ranges: &[(usize, usize)]) -> Vec<String> {
    let authority = expanded.find("://").map_or(0, |at| at + 3);
    let authority_end = expanded[authority..]
        .find(['/', '?', '#'])
        .map_or(expanded.len(), |at| authority + at);
    let host = expanded[authority..authority_end]
        .rfind('@')
        .map_or(authority, |at| authority + at + 1);
    let mut parts = Vec::new();
    for &(start, end) in ranges {
        // Userinfo before the host, then everything after it.
        for (from, to) in [
            (start.max(authority), end.min(host)),
            (start.max(authority_end), end),
        ] {
            if from >= to {
                continue;
            }
            let part = expanded[from..to].trim_matches(['/', '@', ':']);
            if part.len() >= SHORTEST && !parts.iter().any(|known| known == part) {
                parts.push(part.to_string());
            }
        }
    }
    parts
}

/// Hide `parts` in everything printed from now on.
pub fn register(parts: &[String]) {
    let mut secrets = SECRETS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for part in parts {
        if !secrets.contains(part) {
            secrets.push(part.clone());
        }
    }
    // Longest first, so a part that contains another is replaced whole.
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
}

fn mask(text: &str, secrets: &[String]) -> Option<String> {
    if !secrets.iter().any(|secret| text.contains(secret.as_str())) {
        return None;
    }
    let mut masked = text.to_string();
    for secret in secrets {
        masked = masked.replace(secret.as_str(), MASK);
    }
    Some(masked)
}

/// `text` with every registered part replaced by [`MASK`].
pub fn text(text: &str) -> String {
    let secrets = SECRETS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    mask(text, &secrets).unwrap_or_else(|| text.to_string())
}

/// Replace every registered part in the strings of a JSON value.
pub fn value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(string) => {
            let masked = text(string);
            if masked != *string {
                *string = masked;
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(self::value),
        serde_json::Value::Object(fields) => fields.values_mut().for_each(self::value),
        _ => {}
    }
}

/// A log writer that masks each event before it reaches stderr. The
/// tracing formatter hands over one whole event per write.
pub struct Stderr;

impl std::io::Write for Stderr {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let masked = text(&String::from_utf8_lossy(buffer));
        std::io::stderr().write_all(masked.as_bytes())?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(expanded: &str, variable: &str) -> Vec<String> {
        let start = expanded.find(variable).unwrap();
        secret_parts(expanded, &[(start, start + variable.len())])
    }

    #[test]
    fn a_token_in_the_path_or_query_is_a_secret_and_the_host_is_not() {
        assert_eq!(
            parts(
                "https://node.example/v2/tok-0123456789/jsonrpc",
                "tok-0123456789"
            ),
            ["tok-0123456789"]
        );
        assert_eq!(
            parts(
                "https://node.example/rpc?key=tok-0123456789",
                "tok-0123456789"
            ),
            ["tok-0123456789"]
        );
        // A host from a variable stays readable.
        assert!(parts("https://node.example.com/rpc", "node.example.com").is_empty());
        // Short parts are path segments and ports, not keys.
        assert!(parts("http://127.0.0.1:18443/v1", "v1").is_empty());
    }

    #[test]
    fn a_whole_url_from_a_variable_keeps_its_host_and_hides_the_rest() {
        let url = "https://user:hunter2-pass@node.example:8443/v2/tok-0123456789";
        assert_eq!(
            secret_parts(url, &[(0, url.len())]),
            ["user:hunter2-pass", "v2/tok-0123456789"]
        );
        let masked = mask(url, &secret_parts(url, &[(0, url.len())])).unwrap();
        assert_eq!(masked, "https://[redacted]@node.example:8443/[redacted]");
    }

    #[test]
    fn masking_reaches_nested_json_and_leaves_other_text_alone() {
        let secrets = vec!["tok-0123456789".to_string()];
        assert_eq!(mask("nothing to hide", &secrets), None);
        assert_eq!(
            mask(
                "error sending request for url (https://n.example/tok-0123456789)",
                &secrets
            )
            .unwrap(),
            "error sending request for url (https://n.example/[redacted])"
        );
        register(&["json-secret-value".to_string()]);
        let mut envelope = serde_json::json!({
            "result": { "checks": [{ "detail": "at https://n.example/json-secret-value" }] },
            "count": 3
        });
        value(&mut envelope);
        assert_eq!(
            envelope["result"]["checks"][0]["detail"],
            "at https://n.example/[redacted]"
        );
        assert_eq!(envelope["count"], 3);
    }
}
