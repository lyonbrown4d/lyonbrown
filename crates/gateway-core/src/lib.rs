//! Reusable authenticated HTTP gateway; inference and Agent Loop execution remain external.
mod access;
mod audit;
mod config;
mod endpoints;
mod error;
mod http;
mod openapi;
mod request;
mod request_validation;
mod routing;
mod signals;
mod sse;
mod transport;
mod upstream;
mod usage;
pub use access::*;
pub use audit::*;
pub use config::*;
use decision_core::DecisionEngine;
pub use endpoints::*;
pub use error::*;
pub use openapi::ApiDoc;
pub use request::{ChatRequestSchema, RoutingConstraints};
pub use signals::Observations;
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
use tokio::sync::Semaphore;
use utoipa::OpenApi;

struct Inner {
  config: GatewayConfig,
  decision: Arc<DecisionEngine>,
  client: upstream::Client,
  audit: Arc<dyn AuditSink>,
  access: Arc<dyn AccessControl>,
  permits: Arc<Semaphore>,
  ready: AtomicBool,
  signals: Arc<signals::Signals>,
  endpoints: EndpointRegistry,
}
#[derive(Clone)]
pub struct Gateway {
  inner: Arc<Inner>,
}
impl Gateway {
  pub fn new(
    config: GatewayConfig,
    decision: Arc<DecisionEngine>,
    audit: Arc<dyn AuditSink>,
  ) -> anyhow::Result<Self> {
    let access = Arc::new(access::StaticAccess(config.token.clone()));
    Self::new_with_access(config, decision, audit, access)
  }
  /// Production hosts supply an authoritative credential store through this port.
  pub fn new_with_access(
    config: GatewayConfig,
    decision: Arc<DecisionEngine>,
    audit: Arc<dyn AuditSink>,
    access: Arc<dyn AccessControl>,
  ) -> anyhow::Result<Self> {
    config.validate()?;
    agent_security::initialize_tls();
    let client = upstream::Client::new()?;
    let endpoints = EndpointRegistry::new(config.endpoints.clone())?;
    let permits = Arc::new(Semaphore::new(config.limits.max_concurrent));
    Ok(Self {
      inner: Arc::new(Inner {
        config,
        decision,
        client,
        audit,
        access,
        permits,
        ready: AtomicBool::new(true),
        signals: Arc::new(signals::Signals::default()),
        endpoints,
      }),
    })
  }
  pub fn set_ready(&self, ready: bool) {
    self.inner.ready.store(ready, Ordering::Release);
  }
  pub fn is_ready(&self) -> bool {
    self.inner.ready.load(Ordering::Acquire)
  }
  /// Snapshot of nonsecret model IDs; the configured registry is immutable for this instance.
  pub fn models(&self) -> Vec<PublicModel> {
    self
      .inner
      .config
      .models
      .iter()
      .map(|model| PublicModel {
        id: model.id.clone(),
        object: "model",
        created: 0,
        owned_by: "lyonbrown",
      })
      .collect()
  }
  pub fn observations(&self) -> Observations {
    self
      .inner
      .signals
      .snapshot(self.inner.permits.available_permits())
  }
  /// Public read-only schema and Swagger assets contain no live model registry or secrets.
  /// Hosts bind loopback by default; protected operations still require bearer authentication.
  pub fn documentation_router() -> axum::Router {
    utoipa_swagger_ui::SwaggerUi::new("/swagger-ui")
      .url("/api-doc/openapi.json", ApiDoc::openapi())
      .config(
        utoipa_swagger_ui::Config::default()
          .validator_url("none")
          .query_config_enabled(false)
          .persist_authorization(false),
      )
      .into()
  }
  pub fn router(&self) -> axum::Router {
    http::router(self.clone())
  }
}
