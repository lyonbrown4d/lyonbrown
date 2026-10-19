//! Native Ollama JSON/NDJSON fixtures. Metadata is explicitly synthetic.
use crate::{AppConfig, Protocol, Reply};
use agent_test_support::ScriptedReply;
use axum::{Json, http::StatusCode};
use serde_json::{Value, json};

const CREATED: &str = "2026-01-01T00:00:00Z";
pub fn json_reply(
  protocol: Protocol,
  model: &str,
  text: &str,
  tool: Option<(&str, &Value)>,
  done: bool,
) -> Value {
  let mut value = json!({"model":model,"created_at":CREATED,"done":done});
  if protocol == Protocol::OllamaChat {
    let mut message = json!({"role":"assistant","content":text});
    if let Some((name, arguments)) = tool {
      message["tool_calls"] = json!([{"function":{"name":name,"arguments":arguments}}]);
    }
    value["message"] = message;
  } else {
    value["response"] = json!(text);
  }
  if done {
    value["done_reason"] = json!("stop");
    for (key, count) in [
      ("total_duration", 100),
      ("load_duration", 10),
      ("prompt_eval_count", 11),
      ("prompt_eval_duration", 20),
      ("eval_count", 3),
      ("eval_duration", 70),
    ] {
      value[key] = json!(count);
    }
    if protocol == Protocol::OllamaGenerate {
      value["context"] = json!([1, 2, 3]);
    }
  }
  value
}
pub fn scripted(reply: &Reply, protocol: Protocol, model: &str, streaming: bool) -> ScriptedReply {
  if let Reply::Error { status, .. } = reply {
    return ScriptedReply::status(*status, json!({"error":"synthetic model failure"}));
  }
  if matches!(reply, Reply::Stall) {
    return ScriptedReply::stall().header("content-type", "application/x-ndjson");
  }
  if let Reply::Malformed { body } = reply {
    return ScriptedReply::raw_sse(body.as_bytes().to_vec()).header(
      "content-type",
      if streaming {
        "application/x-ndjson"
      } else {
        "application/json"
      },
    );
  }
  let (text, tool) = match reply {
    Reply::Text { text, .. }
    | Reply::Decision { text, .. }
    | Reply::Truncated { text }
    | Reply::MissingTerminal { text } => (text.as_str(), None),
    Reply::Tool {
      name, arguments, ..
    } => ("", Some((name.as_str(), arguments))),
    _ => ("", None),
  };
  if !streaming
    && matches!(
      reply,
      Reply::Truncated { .. } | Reply::MissingTerminal { .. }
    )
  {
    let prefix =
      ScriptedReply::raw_sse(b"{\"model\":".to_vec()).header("content-type", "application/json");
    return if matches!(reply, Reply::MissingTerminal { .. }) {
      prefix.stall_after()
    } else {
      prefix
    };
  }
  if !streaming {
    return ScriptedReply::status(200, json_reply(protocol, model, text, tool, true));
  }
  let prefix = json_reply(protocol, model, text, tool, false);
  let mut body = format!("{prefix}\n");
  if !matches!(
    reply,
    Reply::Truncated { .. } | Reply::MissingTerminal { .. }
  ) {
    body.push_str(&format!(
      "{}\n",
      json_reply(protocol, model, "", None, true)
    ));
  }
  let response =
    ScriptedReply::raw_sse(body.into_bytes()).header("content-type", "application/x-ndjson");
  if matches!(reply, Reply::MissingTerminal { .. }) {
    response.stall_after()
  } else {
    response
  }
}
fn ids(settings: &AppConfig) -> Vec<&str> {
  let mut models: Vec<_> = settings
    .scenarios
    .iter()
    .filter(|scenario| scenario.protocol.is_ollama())
    .map(|scenario| scenario.model.as_str())
    .collect();
  models.sort_unstable();
  models.dedup();
  models
}
pub fn tags(settings: &AppConfig) -> Value {
  json!({"models":ids(settings).into_iter().map(|name|json!({"name":name,"model":name,"modified_at":CREATED,"size":0,"digest":"synthetic-no-weights","details":{"format":"synthetic","family":"mock","families":["mock"],"parameter_size":"0","quantization_level":"NONE"}})).collect::<Vec<_>>()})
}
pub fn show(settings: &AppConfig, model: &str) -> (StatusCode, Json<Value>) {
  if !ids(settings).contains(&model) {
    return (
      StatusCode::NOT_FOUND,
      Json(json!({"error":"synthetic model not found"})),
    );
  }
  let tools = settings.scenarios.iter().any(|scenario| {
    scenario.model == model
      && scenario.protocol == Protocol::OllamaChat
      && scenario
        .replies
        .iter()
        .any(|reply| matches!(reply, Reply::Tool { .. }))
  });
  let capabilities = if tools {
    vec!["completion", "tools"]
  } else {
    vec!["completion"]
  };
  (
    StatusCode::OK,
    Json(
      json!({"license":"Synthetic fixture; no model weights","modelfile":"# Synthetic fixture","parameters":"num_ctx 16384","template":"{{ .Prompt }}","capabilities":capabilities,"model_info":{"general.architecture":"synthetic","synthetic.context_length":16384},"details":{"format":"synthetic","family":"mock","parameter_size":"0","quantization_level":"NONE"}}),
    ),
  )
}
