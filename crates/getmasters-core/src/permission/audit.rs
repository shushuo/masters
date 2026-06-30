//! Audit-log redaction helpers (docs/06 §4: secret-shaped values are redacted before persist).

use serde_json::Value;

const SECRET_KEYS: &[&str] = &[
    "key",
    "token",
    "secret",
    "password",
    "authorization",
    "api_key",
];

fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    SECRET_KEYS.iter().any(|s| k.contains(s))
}

/// Redact secret-shaped values in a JSON args object and return a compact JSON string.
pub fn redact_args(args: &Value) -> String {
    let redacted = redact(args);
    redacted.to_string()
}

fn redact(v: &Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, val)| {
                    if is_secret_key(k) {
                        (k.clone(), Value::String("***".into()))
                    } else {
                        (k.clone(), redact(val))
                    }
                })
                .collect(),
        ),
        Value::Array(arr) => Value::Array(arr.iter().map(redact).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_secret_keys() {
        let v = json!({ "path": "a.txt", "api_key": "sk-123", "nested": { "token": "t" } });
        let out = redact_args(&v);
        assert!(out.contains("a.txt"));
        assert!(!out.contains("sk-123"));
        assert!(!out.contains("\"t\""));
        assert!(out.contains("***"));
    }
}
