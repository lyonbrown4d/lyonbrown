//! Mandatory content protection shared by the HTTP and in-process entry points.
use crate::{AccessPrincipal, GatewayConfig, GatewayError};
use agent_privacy::PrivacyService;
use async_trait::async_trait;
use axum::http::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

/// A policy receives authenticated identity and the raw request before routing.
#[async_trait]
pub trait ContentPolicy: Send + Sync {
  async fn protect(
    &self,
    principal: &AccessPrincipal,
    token: SecretString,
    body: Value,
  ) -> Result<Value, GatewayError>;
}

pub(crate) struct RuleContentPolicy {
  service: Arc<Mutex<PrivacyService>>,
  shared: Option<Arc<tokio::sync::Mutex<PrivacyService>>>,
  permits: Arc<Semaphore>,
}
impl RuleContentPolicy {
  pub fn new(
    config: &GatewayConfig,
    shared: Option<Arc<tokio::sync::Mutex<PrivacyService>>>,
  ) -> anyhow::Result<Self> {
    let mut known = vec![SecretString::from(config.token.expose_secret().to_owned())];
    known.extend(
      config
        .models
        .iter()
        .filter_map(|model| model.api_key.as_ref())
        .map(|key| SecretString::from(key.expose_secret().to_owned())),
    );
    known.extend(
      config
        .endpoints
        .iter()
        .filter_map(|endpoint| endpoint.api_key.as_ref())
        .map(|key| SecretString::from(key.expose_secret().to_owned())),
    );
    let service = PrivacyService::new(&known)?;
    Ok(Self {
      service: Arc::new(Mutex::new(service)),
      shared,
      permits: Arc::new(Semaphore::new(config.limits.max_concurrent)),
    })
  }
}
#[async_trait]
impl ContentPolicy for RuleContentPolicy {
  async fn protect(
    &self,
    principal: &AccessPrincipal,
    token: SecretString,
    mut body: Value,
  ) -> Result<Value, GatewayError> {
    let permit = self
      .permits
      .clone()
      .try_acquire_owned()
      .map_err(|_| unavailable())?;
    let service = self.service.clone();
    let shared = self.shared.clone();
    let scope = format!(
      "{}:{}:{}",
      principal.project_id.len(),
      principal.project_id,
      principal.key_id
    );
    tokio::task::spawn_blocking(move || {
      let _permit = permit;
      let images = crate::inline_images::InlineImages::take(&mut body)?;
      if let Some(shared) = shared {
        let mut privacy = shared.blocking_lock();
        privacy
          .register_secret(&scope, token.expose_secret())
          .map_err(|_| unavailable())?;
        protect_content(&mut privacy, &scope, &mut body).map_err(|_| rejected())?;
      }
      let mut privacy = service.lock().map_err(|_| unavailable())?;
      privacy
        .register_secret(&scope, token.expose_secret())
        .map_err(|_| unavailable())?;
      protect_content(&mut privacy, &scope, &mut body).map_err(|_| rejected())?;
      images.restore(&mut body)?;
      Ok(body)
    })
    .await
    .map_err(|_| unavailable())?
  }
}
fn rejected() -> GatewayError {
  GatewayError(
    StatusCode::UNPROCESSABLE_ENTITY,
    "privacy_rejected",
    "Content cannot be safely sent to this model",
  )
}
fn unavailable() -> GatewayError {
  GatewayError(
    StatusCode::SERVICE_UNAVAILABLE,
    "privacy_unavailable",
    "Privacy processing is unavailable",
  )
}
fn protect_content(privacy: &mut PrivacyService, scope: &str, body: &mut Value) -> Result<(), ()> {
  if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
    for message in messages {
      if let Some(content) = message.get_mut("content") {
        if let Some(parts) = content.as_array() {
          for part in parts {
            if part
              .get("type")
              .and_then(Value::as_str)
              .is_some_and(|kind| kind != "text")
            {
              return Err(());
            }
          }
        }
        protect_value(privacy, scope, content, 0)?;
      }
      if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for call in calls {
          if let Some(arguments) = call.pointer_mut("/function/arguments") {
            let text = arguments.as_str().ok_or(())?;
            let mut value: Value = serde_json::from_str(text).map_err(|_| ())?;
            protect_value(privacy, scope, &mut value, 0)?;
            check_value(privacy, scope, &value, 0)?;
            *arguments = Value::String(serde_json::to_string(&value).map_err(|_| ())?);
          }
        }
      }
    }
  }
  // Unmodified protocol fields and schemas must not contain protected material.
  check_value(privacy, scope, body, 0)
}
fn check_value(
  privacy: &mut PrivacyService,
  scope: &str,
  value: &Value,
  depth: usize,
) -> Result<(), ()> {
  if depth > 32 {
    return Err(());
  }
  match value {
    Value::String(text) => {
      if privacy.protect_text(scope, text).map_err(|_| ())? != *text {
        return Err(());
      }
    }
    Value::Array(values) => {
      for value in values {
        check_value(privacy, scope, value, depth + 1)?;
      }
    }
    Value::Object(object) => {
      for (key, value) in object {
        if privacy.protect_text(scope, key).map_err(|_| ())? != *key {
          return Err(());
        }
        check_value(privacy, scope, value, depth + 1)?;
      }
    }
    _ => {}
  }
  Ok(())
}
fn protect_value(
  privacy: &mut PrivacyService,
  scope: &str,
  value: &mut Value,
  depth: usize,
) -> Result<(), ()> {
  if depth > 32 {
    return Err(());
  }
  match value {
    Value::String(text) => *text = privacy.protect_text(scope, text).map_err(|_| ())?,
    Value::Array(values) => {
      for value in values {
        protect_value(privacy, scope, value, depth + 1)?;
      }
    }
    Value::Object(object) => {
      for (key, value) in object {
        if key != "type" {
          protect_value(privacy, scope, value, depth + 1)?;
        }
      }
    }
    _ => {}
  }
  Ok(())
}
