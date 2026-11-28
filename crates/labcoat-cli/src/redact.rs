//! Masks private RPC endpoints in diagnostics, keeping the scheme and host
//! visible. Match whole URLs first so short credentials are removed without
//! masking the same text elsewhere. The longer interpolated components can
//! also be masked when diagnostics include them outside the full URL.

use std::sync::Mutex;

pub const MASK: &str = "[redacted]";

/// A part shorter than this is not hidden: it is a port or a path segment
/// such as `v1`, and masking every `v1` in the output would garble it.
const SHORTEST: usize = 8;

static SECRETS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static ENDPOINTS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn url_authority(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    Some((scheme, authority))
}

pub fn has_url_credentials(url: &str) -> bool {
    url_authority(url).is_some_and(|(_, authority)| authority.contains('@'))
        || url.contains(['?', '#'])
}

/// Keep the host visible in diagnostics and hide the URL's private parts.
pub fn endpoint_label(url: &str) -> String {
    let Some((scheme, authority)) = url_authority(url) else {
        return MASK.to_string();
    };
    let host = authority.rsplit('@').next().unwrap_or_default();
    format!("{scheme}://{host}/{MASK}")
}

/// Redact a complete endpoint in diagnostics even when a credential is
/// too short to replace safely in arbitrary output.
pub fn register_endpoint(url: &str) {
    let label = endpoint_label(url);
    if url.is_empty() || label.starts_with(url) {
        return;
    }
    let mut endpoints = ENDPOINTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !endpoints.iter().any(|(known, _)| known == url) {
        endpoints.push((url.to_string(), label));
        endpoints.sort_by_key(|(url, _)| std::cmp::Reverse(url.len()));
    }
}

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
    let endpoints = ENDPOINTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut output = text.to_string();
    for (url, label) in endpoints.iter() {
        if output.contains(url) {
            output = output.replace(url, label);
        }
    }
    mask(&output, &secrets).unwrap_or(output)
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
