use serde_json::Value;

pub(super) fn reflected<S: AsRef<str>>(value: &Value, secrets: &[S]) -> bool {
    let contains = |text: &str| {
        secrets
            .iter()
            .any(|secret| !secret.as_ref().is_empty() && text.contains(secret.as_ref()))
    };
    match value {
        Value::String(value) => contains(value),
        Value::Number(value) => contains(&value.to_string()),
        Value::Array(values) => values.iter().any(|value| reflected(value, secrets)),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| contains(key) || reflected(value, secrets)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn all_integrations_reject_reflected_secrets_in_keys_text_and_numbers() {
        let secrets = ["synthetic-private-token", "2000000000"];
        for value in [
            json!({"synthetic-private-token": null}),
            json!({"outer":[{"label":"synthetic-private-token"}]}),
            json!({"outer":[{"metadata":2000000000u64}]}),
            serde_json::from_str(r#"{"label":"\u0073ynthetic-private-token"}"#).unwrap(),
        ] {
            assert!(reflected(&value, &secrets));
        }
        assert!(!reflected(&json!({"public":25}), &secrets));
        assert!(!reflected(&json!({"public":"text"}), &[""]));
    }
}
