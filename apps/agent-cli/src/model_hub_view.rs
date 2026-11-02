//! Read the model hub's JSON replies as text a person can scan.
//!
//! The daemon returns each hub action's native JSON. Parsing stays here so the client keeps no
//! dependency on hub types and gains no authority: this module only formats strings.

use serde_json::Value;

/// Formats one hub reply. Unknown shapes fall back to indented JSON instead of losing data.
pub fn format(action: &str, json: &str) -> String {
  let Ok(value) = serde_json::from_str::<Value>(json) else {
    return json.to_owned();
  };
  match action {
    "search" => list_models(&value),
    "inspect" => model_detail(&value),
    "plan" => plan_detail(&value),
    "installed" => installed(&value),
    "status" => job_state(&value),
    _ => receipt(action, &value),
  }
}

fn list_models(value: &Value) -> String {
  let Some(models) = value.as_array() else {
    return detail_or_fallback(value);
  };
  if models.is_empty() {
    return "没有匹配的模型。".into();
  }
  let mut lines = vec![format!("找到 {} 个模型", models.len())];
  for model in models {
    lines.push(format!(
      "  {} · {} · {} · {} · {}",
      text(model, "repository"),
      text(model, "revision"),
      text(model, "architecture"),
      if model["gated"].as_bool().unwrap_or(false) {
        "需授权"
      } else {
        "公开"
      },
      files_summary(model)
    ));
  }
  lines.join("\n")
}

/// A single object renders as a model detail only when it looks like one.
fn detail_or_fallback(value: &Value) -> String {
  if value.get("repository").is_some() {
    model_detail(value)
  } else {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
  }
}

fn model_detail(value: &Value) -> String {
  let mut lines = vec![
    format!("仓库 · {}", text(value, "repository")),
    format!("版本 · {}", text(value, "revision")),
    format!("架构 · {}", text(value, "architecture")),
    format!(
      "许可 · {}",
      match value["license"].as_str() {
        Some(license) if !license.is_empty() => license.to_owned(),
        _ => "未标注".into(),
      }
    ),
  ];
  if value["gated"].as_bool().unwrap_or(false) {
    lines.push("访问 · 需要在来源站点授权".into());
  }
  lines.push(format!("文件 · {}", files_summary(value)));
  if let Some(files) = value["files"].as_array() {
    for file in files.iter().take(8) {
      lines.push(format!(
        "  {} · {}",
        text(file, "path"),
        human_bytes(file["bytes"].as_u64().unwrap_or(0))
      ));
    }
    if files.len() > 8 {
      lines.push(format!("  … 另有 {} 个文件", files.len() - 8));
    }
  }
  lines.join("\n")
}

fn plan_detail(value: &Value) -> String {
  let mut lines = vec![
    format!("计划 · {}", text(value, "id")),
    format!("仓库 · {}", text(value, "repository")),
    format!("版本 · {}", text(value, "revision")),
    format!("运行时 · {}", text(value, "runtime")),
    format!("格式 · {}", text(value, "format")),
    format!("入口 · {}", text(value, "entrypoint")),
    format!(
      "大小 · {}",
      human_bytes(value["total_bytes"].as_u64().unwrap_or(0))
    ),
  ];
  let compatibility = text(value, "compatibility");
  if !compatibility.is_empty() {
    lines.push(format!("兼容性 · {compatibility}"));
  }
  lines.push("确认后执行 /models install REPO REVISION FILE RUNTIME PLAN_ID".into());
  lines.join("\n")
}

fn installed(value: &Value) -> String {
  let Some(models) = value.as_array() else {
    return detail_or_fallback(value);
  };
  if models.is_empty() {
    return "没有已安装的模型。".into();
  }
  let mut lines = vec![format!("已安装 {} 个模型", models.len())];
  for model in models {
    lines.push(format!(
      "  {} · {} · {} · {}",
      text(model, "state"),
      text(&model["plan"], "runtime"),
      text(model, "directory"),
      text(model, "entrypoint")
    ));
  }
  lines.join("\n")
}

fn job_state(value: &Value) -> String {
  let mut lines = vec![
    format!("模型 · {}", text(value, "id")),
    format!("状态 · {}", text(value, "state")),
  ];
  if let Some(downloaded) = value["downloaded_bytes"].as_u64() {
    lines.push(format!("已下载 · {}", human_bytes(downloaded)));
  }
  if let Some(reason) = value["reason"].as_str()
    && !reason.is_empty()
  {
    lines.push(format!("原因 · {reason}"));
  }
  lines.join("\n")
}

/// Install, load and removal replies carry only an identifier and a verb.
fn receipt(action: &str, value: &Value) -> String {
  let mut lines = vec![format!("Models · {action}")];
  if let Some(map) = value.as_object() {
    for (key, entry) in map {
      let rendered = match entry {
        Value::String(text) => text.clone(),
        other => other.to_string(),
      };
      lines.push(format!("{key} · {rendered}"));
    }
  }
  if lines.len() == 1 {
    lines.push(value.to_string());
  }
  lines.join("\n")
}

fn files_summary(value: &Value) -> String {
  let Some(files) = value["files"].as_array() else {
    return "文件未知".into();
  };
  let total: u64 = files
    .iter()
    .map(|file| file["bytes"].as_u64().unwrap_or(0))
    .sum();
  let bytes = value["total_bytes"].as_u64().unwrap_or(total);
  format!("{} 个文件 · {}", files.len(), human_bytes(bytes))
}

fn text(value: &Value, key: &str) -> String {
  value[key]
    .as_str()
    .filter(|text| !text.is_empty())
    .unwrap_or("—")
    .to_owned()
}

fn human_bytes(bytes: u64) -> String {
  const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
  let mut size = bytes as f64;
  let mut unit = 0;
  while size >= 1024.0 && unit + 1 < UNITS.len() {
    size /= 1024.0;
    unit += 1;
  }
  if unit == 0 {
    format!("{bytes} B")
  } else {
    format!("{size:.1} {}", UNITS[unit])
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn search_lists_repository_revision_and_size() {
    let json = r#"[{"repository":"org/model","revision":"main","architecture":"llama","gated":false,
      "files":[{"path":"a.gguf","bytes":1048576},{"path":"b.gguf","bytes":1048576}]}]"#;
    let text = format("search", json);
    assert!(text.contains("org/model"));
    assert!(text.contains("llama"));
    assert!(text.contains("公开"));
    assert!(text.contains("2 个文件"));
    assert!(text.contains("2.0 MB"));
  }

  #[test]
  fn install_plan_reports_the_entrypoint_and_next_step() {
    let json = r#"{"id":"plan-1","repository":"org/model","revision":"main","runtime":"llama_cpp",
      "format":"gguf","entrypoint":"model.gguf","files":[],"total_bytes":524288000,
      "compatibility":"needs load test"}"#;
    let text = format("plan", json);
    assert!(text.contains("plan-1"));
    assert!(text.contains("model.gguf"));
    assert!(text.contains("兼容性 · needs load test"));
    assert!(text.contains("/models install"));
  }

  #[test]
  fn installed_and_status_stay_readable() {
    let installed = r#"[{"state":"verified","directory":"/m","entrypoint":"/m/model.gguf",
      "plan":{"runtime":"llama_cpp"},"verified_files":[]}]"#;
    let text = format("installed", installed);
    assert!(text.contains("verified"));
    assert!(text.contains("/m/model.gguf"));

    let status = format(
      "status",
      r#"{"id":"job-1","state":"downloading","downloaded_bytes":1048576}"#,
    );
    assert!(status.contains("downloading"));
    assert!(status.contains("1.0 MB"));
  }

  #[test]
  fn unknown_shapes_fall_back_to_json_instead_of_dropping_data() {
    let json = r#"{"anything":1}"#;
    assert!(format("search", json).contains("anything"));
    assert!(format("remove", json).contains("anything"));
    assert_eq!(format("search", "not json"), "not json");
  }
}
