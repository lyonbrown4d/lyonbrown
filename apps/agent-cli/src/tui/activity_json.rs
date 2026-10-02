use serde_json::Value;

use super::terminal_safe;

const MAX_JSON_DEPTH: usize = 6;
const MAX_JSON_ITEMS: usize = 12;
const MAX_JSON_CHARS: usize = 1_024;
const MAX_STRING_SCAN_CHARS: usize = 1_100;

pub(crate) fn bounded_json(value: &Value, truncated_suffix: &str) -> String {
  let mut output = String::new();
  let mut budget = JsonBudget {
    remaining: MAX_JSON_CHARS,
    truncated: false,
  };
  write_value(value, 0, &mut output, &mut budget);
  if budget.truncated {
    append_suffix(&mut output, truncated_suffix);
  }
  output
}

struct JsonBudget {
  remaining: usize,
  truncated: bool,
}

fn write_value(value: &Value, depth: usize, output: &mut String, budget: &mut JsonBudget) {
  if budget.remaining == 0 {
    budget.truncated = true;
    return;
  }
  if depth >= MAX_JSON_DEPTH {
    budget.truncated = true;
    push_budgeted(output, "…", budget);
    return;
  }
  match value {
    Value::Null => push_budgeted(output, "null", budget),
    Value::Bool(value) => push_budgeted(output, if *value { "true" } else { "false" }, budget),
    Value::Number(value) => push_budgeted(output, &value.to_string(), budget),
    Value::String(value) => push_sanitized_string(output, value, budget),
    Value::Array(values) => {
      push_budgeted(output, "[", budget);
      for (index, value) in values.iter().take(MAX_JSON_ITEMS).enumerate() {
        if index > 0 {
          push_budgeted(output, ", ", budget);
        }
        write_value(value, depth + 1, output, budget);
      }
      budget.truncated |= values.len() > MAX_JSON_ITEMS;
      push_budgeted(output, "]", budget);
    }
    Value::Object(values) => {
      push_budgeted(output, "{", budget);
      for (index, (key, value)) in values.iter().take(MAX_JSON_ITEMS).enumerate() {
        if index > 0 {
          push_budgeted(output, ", ", budget);
        }
        push_sanitized_string(output, key, budget);
        push_budgeted(output, ": ", budget);
        write_value(value, depth + 1, output, budget);
      }
      budget.truncated |= values.len() > MAX_JSON_ITEMS;
      push_budgeted(output, "}", budget);
    }
  }
}

fn push_sanitized_string(output: &mut String, value: &str, budget: &mut JsonBudget) {
  let scanned = value
    .chars()
    .take(MAX_STRING_SCAN_CHARS)
    .collect::<String>();
  budget.truncated |= value.chars().nth(MAX_STRING_SCAN_CHARS).is_some();
  let safe = terminal_safe(&scanned);
  let encoded = serde_json::to_string(&safe).expect("serializing a string cannot fail");
  push_budgeted(output, &encoded, budget);
}

fn push_budgeted(output: &mut String, value: &str, budget: &mut JsonBudget) {
  let mut written = 0;
  for character in value.chars().take(budget.remaining) {
    output.push(character);
    written += 1;
  }
  budget.remaining = budget.remaining.saturating_sub(written);
  budget.truncated |= written < value.chars().count();
}

fn append_suffix(output: &mut String, suffix: &str) {
  let suffix_len = suffix.chars().count().min(MAX_JSON_CHARS);
  let keep = MAX_JSON_CHARS.saturating_sub(suffix_len);
  *output = output.chars().take(keep).collect();
  output.extend(suffix.chars().take(suffix_len));
}
