//! Optional loopback listener boundary. Internal credentials are never accepted here.
use std::sync::Arc;

use agent_secrets::ExposeSecret;
use axum::{
  Router,
  extract::{Request, State},
  http::{StatusCode, header},
  middleware::{self, Next},
  response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};

struct HttpState {
  token_digest: [u8; 32],
}

impl crate::Daemon {
  pub async fn gateway_shutdown(&self) {
    self.stopping.cancelled().await;
  }
  /// No socket is opened here. The process owns binding and draining.
  pub fn gateway_router(&self) -> anyhow::Result<Option<Router>> {
    if self.config.gateway.http_listen.is_none() {
      return Ok(None);
    }
    let gateway = self
      .gateway
      .gateway
      .as_ref()
      .ok_or_else(|| anyhow::anyhow!("gateway has no configured models"))?;
    let token = self
      .config
      .gateway
      .http_token_env
      .as_ref()
      .and_then(|name| self.config.gateway_credentials.get(name))
      .ok_or_else(|| anyhow::anyhow!("gateway HTTP credential unavailable"))?;
    let state = Arc::new(HttpState {
      token_digest: Sha256::digest(token.expose_secret().as_bytes()).into(),
    });
    Ok(Some(
      gateway
        .router()
        .layer(middleware::from_fn_with_state(state, authenticate)),
    ))
  }
}

async fn authenticate(
  State(state): State<Arc<HttpState>>,
  request: Request,
  next: Next,
) -> Response {
  let supplied = request
    .headers()
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));
  let valid = supplied
    .filter(|value| value.len() <= 4096)
    .is_some_and(|token| {
      use subtle::ConstantTimeEq;
      let supplied: [u8; 32] = Sha256::digest(token.as_bytes()).into();
      bool::from(supplied.ct_eq(&state.token_digest))
    });
  if !valid
    || request
      .headers()
      .get_all(header::AUTHORIZATION)
      .iter()
      .count()
      != 1
  {
    return (
      StatusCode::UNAUTHORIZED,
      "Model API authentication required",
    )
      .into_response();
  }
  next.run(request).await
}
