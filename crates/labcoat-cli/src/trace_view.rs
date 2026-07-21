use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceLine {
    pub depth: usize,
    pub summary: String,
    pub raw: String,
}

pub fn normalize(value: &Value) -> Vec<TraceLine> {
    let mut lines = Vec::new();
    match value {
        Value::Null => lines.push(TraceLine {
            depth: 0,
            summary: "No trace events".into(),
            raw: "null".into(),
        }),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let is_wrapper = item.get("trace").and_then(Value::as_array).is_some();
                if items.len() > 1 || is_wrapper {
                    lines.push(TraceLine {
                        depth: 0,
                        summary: format!("Protostone {}", index + 1),
                        raw: compact(item),
                    });
                    walk(item, 1, &mut lines);
                } else {
                    walk(item, 0, &mut lines);
                }
            }
        }
        other => walk(other, 0, &mut lines),
    }
    lines
}

fn walk(value: &Value, depth: usize, lines: &mut Vec<TraceLine>) {
    match value {
        Value::Array(items) => {
            for item in items {
                walk(item, depth, lines);
            }
        }
        Value::Object(map) => {
            if let Some(trace) = map.get("trace").and_then(Value::as_array) {
                for item in trace {
                    walk(item, depth, lines);
                }
                return;
            }
            lines.push(TraceLine {
                depth,
                summary: event_summary(value),
                raw: serde_json::to_string_pretty(value).unwrap_or_else(|_| compact(value)),
            });
        }
        primitive => lines.push(TraceLine {
            depth,
            summary: compact(primitive),
            raw: compact(primitive),
        }),
    }
}

fn event_summary(value: &Value) -> String {
    let kind = value
        .get("type")
        .or_else(|| value.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("event");
    match kind {
        "call" => {
            let caller = alkane_id(value.get("caller"));
            let target = alkane_id(value.get("target"));
            let inputs = value
                .get("inputs")
                .and_then(Value::as_array)
                .map(|values| values.iter().map(compact).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            let fuel = value
                .get("fuel_allocated")
                .or_else(|| value.get("fuelAllocated"))
                .map(compact);
            let mut summary = format!("call {caller} -> {target}");
            if !inputs.is_empty() {
                summary.push_str(&format!("  inputs [{inputs}]"));
            }
            if let Some(fuel) = fuel {
                summary.push_str(&format!("  fuel {fuel}"));
            }
            summary
        }
        "return" => {
            let data = value
                .get("return_data")
                .or_else(|| value.get("returnData"))
                .or_else(|| value.pointer("/data/response/data"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let decoded = decode_return(data);
            let fuel = value
                .get("fuel_used")
                .or_else(|| value.get("fuelUsed"))
                .map(compact)
                .unwrap_or_else(|| "0".into());
            if decoded.is_empty() {
                format!("return  fuel used {fuel}")
            } else {
                format!("return {decoded}  fuel used {fuel}")
            }
        }
        "revert" => {
            let reason = value
                .get("error_message")
                .or_else(|| value.pointer("/data/error_message"))
                .and_then(Value::as_str)
                .unwrap_or("contract reverted");
            format!("revert: {reason}")
        }
        "create_alkane" | "create" => {
            let id = value
                .get("alkane_id")
                .or_else(|| value.get("new_alkane"))
                .or_else(|| value.get("data"));
            format!("create alkane {}", alkane_id(id))
        }
        other => {
            let transfers = value
                .get("alkane_transfers")
                .or_else(|| value.get("alkaneTransfers"))
                .and_then(Value::as_array)
                .map(Vec::len);
            match transfers {
                Some(count) => format!("{other}  {count} transfer(s)"),
                None => other.replace('_', " "),
            }
        }
    }
}

fn alkane_id(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return "?".into();
    };
    match (
        value.get("block").map(compact),
        value.get("tx").map(compact),
    ) {
        (Some(block), Some(tx)) => format!("{block}:{tx}"),
        _ => compact(value),
    }
}

pub fn decoded_return_from_traces(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if map
                .get("type")
                .or_else(|| map.get("event"))
                .and_then(Value::as_str)
                == Some("return")
            {
                let data = map
                    .get("return_data")
                    .or_else(|| map.get("returnData"))
                    .or_else(|| value.pointer("/data/response/data"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let decoded = decode_return(data);
                if !decoded.is_empty() {
                    return Some(decoded);
                }
            }
            map.values().find_map(decoded_return_from_traces)
        }
        Value::Array(items) => items.iter().find_map(decoded_return_from_traces),
        _ => None,
    }
}

pub fn decode_return(value: &str) -> String {
    let hex = value.strip_prefix("0x").unwrap_or(value);
    if hex.is_empty() {
        return String::new();
    }
    let Ok(bytes) = hex::decode(hex) else {
        return value.to_string();
    };
    if let Ok(text) = std::str::from_utf8(&bytes) {
        if !text.is_empty()
            && text
                .chars()
                .all(|character| (' '..='~').contains(&character) || character.is_whitespace())
        {
            return format!("\"{text}\"");
        }
    }
    if bytes.len() <= 16 {
        let mut padded = [0_u8; 16];
        padded[..bytes.len()].copy_from_slice(&bytes);
        return u128::from_le_bytes(padded).to_string();
    }
    format!("0x{hex}")
}

fn compact(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "?".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_little_endian_trace_return() {
        assert_eq!(decode_return("03000000000000000000000000000000"), "3");
    }

    #[test]
    fn normalizes_call_and_return() {
        let value = serde_json::json!([{"trace": [
            {"type":"call","caller":{"block":0,"tx":0},"target":{"block":2,"tx":8},"inputs":[1]},
            {"type":"return","return_data":"03000000000000000000000000000000","fuel_used":7}
        ]}]);
        let lines = normalize(&value);
        assert!(lines
            .iter()
            .any(|line| line.summary.contains("call 0:0 -> 2:8")));
        assert!(lines.iter().any(|line| line.summary.contains("return 3")));
    }
}
