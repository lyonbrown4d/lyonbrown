use serde_json::Value;

pub(super) fn safe(value: &str) -> String {
  crate::view_model::safe_text(value)
}

pub(super) fn short(value: &str, limit: usize) -> String {
  let value = safe(value).split_whitespace().collect::<Vec<_>>().join(" ");
  let mut result: String = value.chars().take(limit).collect();
  if value.chars().count() > limit {
    result.push('…');
  }
  result
}

pub(super) fn details(value: &Value) -> String {
  fn redact(value: &Value) -> Value {
    match value {
      Value::Object(fields) => Value::Object(
        fields
          .iter()
          .map(|(key, value)| {
            let normalized = key.to_ascii_lowercase().replace(['_', '-'], "");
            let secret = normalized == "token"
              || [
                "password",
                "secret",
                "credential",
                "authorization",
                "apikey",
                "accesstoken",
                "refreshtoken",
                "cookie",
              ]
              .iter()
              .any(|part| normalized.contains(part));
            (
              safe(key),
              if secret {
                Value::String("[已隐藏]".into())
              } else {
                redact(value)
              },
            )
          })
          .collect(),
      ),
      Value::Array(values) => Value::Array(values.iter().map(redact).collect()),
      Value::String(value) => Value::String(safe(value)),
      value => value.clone(),
    }
  }
  // Bound retained display data; full execution output remains daemon-owned.
  let encoded = serde_json::to_string_pretty(&redact(value)).unwrap_or_default();
  bounded(&encoded)
}

pub(super) fn bounded(value: &str) -> String {
  let mut value = safe(value);
  if let Some((index, _)) = value.char_indices().nth(16_384) {
    value.truncate(index);
    value.push_str("\n…显示已截断");
  }
  value
}

pub(super) fn target(arguments: &Value) -> String {
  for key in [
    "path",
    "file_path",
    "filePath",
    "command",
    "url",
    "query",
    "pattern",
    "tool",
  ] {
    if let Some(value) = arguments.get(key).and_then(Value::as_str) {
      return short(value, 100);
    }
  }
  String::new()
}
