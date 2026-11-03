use crate::{Result, Runtime, RuntimeError, RuntimeTool, ToolCall};
use serde_json::Value;

impl Runtime {
  pub(crate) async fn protect_text(&self, text: &str) -> Result<String> {
    let mut privacy = self.privacy.lock().await;
    let protected = privacy
      .protect_text(&self.config.session_id, text)
      .map_err(|_| RuntimeError::Execution("privacy protection failed".into()))?;
    let saved = privacy
      .export_scope(&self.config.session_id)
      .map_err(|_| self.session.private_scope_failed())?;
    self.session.commit_private_scope(saved).await?;
    Ok(protected)
  }

  /// Protect both summary inputs using the session's durable mapping boundary.
  pub async fn protect_summary_input(
    &self,
    system: &str,
    transcript: &str,
  ) -> Result<(String, String)> {
    let system = self.protect_text(system).await?;
    let transcript = self.protect_text(transcript).await?;
    Ok((system, transcript))
  }

  /// Refuse summary content that would expose a detected sensitive value.
  pub async fn validate_summary_output(&self, output: &str) -> Result<()> {
    self
      .validate_model_value(&Value::String(output.to_owned()))
      .await
  }

  /// Resolve only whole-value references, after the registered host tool approves the sink.
  pub(crate) async fn authorize_tool_call(
    &self,
    tool: &dyn RuntimeTool,
    call: &ToolCall,
  ) -> Result<ToolCall> {
    let mut resolved = call.clone();
    let mut used_reference = false;
    let privacy = self.privacy.lock().await;
    fn visit(
      value: &mut Value,
      tool: &dyn RuntimeTool,
      call: &ToolCall,
      privacy: &agent_privacy::PrivacyService,
      scope: &str,
      used_reference: &mut bool,
    ) -> Result<()> {
      match value {
        Value::String(text) if text.contains("[PRIVATE:") => {
          if !tool.authorize_sensitive_reference(call, text) {
            return Err(RuntimeError::Tool(
              "sensitive reference is not authorized for this sink".into(),
            ));
          }
          *text = privacy
            .resolve(scope, text)
            .map_err(|_| RuntimeError::Tool("sensitive reference is unavailable".into()))?
            .to_owned();
          *used_reference = true;
        }
        Value::Array(values) => {
          for item in values {
            visit(item, tool, call, privacy, scope, used_reference)?;
          }
        }
        Value::Object(values) => {
          if values.keys().any(|key| key.contains("[PRIVATE:")) {
            return Err(RuntimeError::Tool(
              "sensitive references cannot be object keys".into(),
            ));
          }
          for item in values.values_mut() {
            visit(item, tool, call, privacy, scope, used_reference)?;
          }
        }
        _ => {}
      }
      Ok(())
    }
    visit(
      &mut resolved.arguments,
      tool,
      call,
      &privacy,
      &self.config.session_id,
      &mut used_reference,
    )?;
    if used_reference && !tool.authorize_resolved_sensitive_call(call, &resolved) {
      return Err(RuntimeError::Tool(
        "resolved sensitive call is not authorized".into(),
      ));
    }
    Ok(resolved)
  }

  /// Reject provider content containing detected secrets before it enters opaque Rig state.
  pub(crate) async fn validate_model_value(&self, value: &Value) -> Result<()> {
    fn visit(
      privacy: &mut agent_privacy::PrivacyService,
      scope: &str,
      value: &Value,
    ) -> Result<()> {
      match value {
        Value::String(text) => {
          let protected = privacy
            .protect_text(scope, text)
            .map_err(|_| RuntimeError::Execution("privacy validation failed".into()))?;
          if protected != *text {
            return Err(RuntimeError::Execution(
              "model output contained sensitive content".into(),
            ));
          }
        }
        Value::Array(values) => {
          for item in values {
            visit(privacy, scope, item)?;
          }
        }
        Value::Object(values) => {
          for (key, item) in values {
            let normalized = key.to_ascii_lowercase().replace('-', "_");
            if matches!(
              normalized.as_str(),
              "password" | "passwd" | "pwd" | "api_key" | "access_token" | "secret"
            ) && matches!(item, Value::String(value) if !value.is_empty() && !is_reference(value))
            {
              return Err(RuntimeError::Execution(
                "model output contained sensitive content".into(),
              ));
            }
            visit(privacy, scope, &Value::String(key.clone()))?;
            visit(privacy, scope, item)?;
          }
        }
        _ => {}
      }
      Ok(())
    }
    let mut privacy = self.privacy.lock().await;
    visit(&mut privacy, &self.config.session_id, value)
  }
}

fn is_reference(value: &str) -> bool {
  value
    .strip_prefix("[PRIVATE:")
    .and_then(|s| s.strip_suffix(']'))
    .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}
