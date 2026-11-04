//! Reusable authenticated HTTP gateway; inference and Agent Loop execution remain external.
mod access;
mod audit;
mod config;
mod endpoints;
mod error;
mod http;
mod inline_images;
mod openapi;
mod privacy;
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
pub use privacy::ContentPolicy;
pub use request::{ChatRequestSchema, RoutingConstraints};
pub use signals::Observations;
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
use tokio::sync::Semaphore;

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
  privacy: privacy::RuleContentPolicy,
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
    Self::build(config, decision, audit, access, None)
  }
  /// Shares host credential recognition with sessions while retaining Gateway rules.
  pub fn new_with_access_and_privacy(
    config: GatewayConfig,
    decision: Arc<DecisionEngine>,
    audit: Arc<dyn AuditSink>,
    access: Arc<dyn AccessControl>,
    privacy: Arc<tokio::sync::Mutex<agent_privacy::PrivacyService>>,
  ) -> anyhow::Result<Self> {
    Self::build(config, decision, audit, access, Some(privacy))
  }
  fn build(
    config: GatewayConfig,
    decision: Arc<DecisionEngine>,
    audit: Arc<dyn AuditSink>,
    access: Arc<dyn AccessControl>,
    shared_privacy: Option<Arc<tokio::sync::Mutex<agent_privacy::PrivacyService>>>,
  ) -> anyhow::Result<Self> {
    config.validate()?;
    agent_security::initialize_tls();
    let privacy = privacy::RuleContentPolicy::new(&config, shared_privacy)?;
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
        privacy,
      }),
    })
  }
  pub fn set_ready(&self, ready: bool) {
    self.inner.ready.store(ready, Ordering::Release);
  }
  pub fn is_ready(&self) -> bool {
    self.inner.ready.load(Ordering::Acquire)
  }
  /// Snapshot of nonsecret model and eligible group IDs for an unrestricted caller.
  pub fn models(&self) -> Vec<PublicModel> {
    self.models_for(&AccessPrincipal::legacy())
  }
  /// Lists accessible models and groups without revealing group membership.
  pub fn models_for(&self, principal: &AccessPrincipal) -> Vec<PublicModel> {
    let mut constraints = self.inner.config.constraints.clone();
    constraints.required_capabilities.insert("text".into());
    let model = |id: &str| PublicModel {
      id: id.to_owned(),
      object: "model",
      created: 0,
      owned_by: "lyonbrown",
    };
    let mut models: Vec<_> = self
      .inner
      .config
      .models
      .iter()
      .filter(|item| principal.permits_model(&item.id))
      .map(|item| model(&item.id))
      .collect();
    models.extend(
      self
        .inner
        .config
        .groups
        .iter()
        .filter(|group| {
          group.models.iter().any(|id| {
            self.inner.config.models.iter().any(|item| {
              &item.id == id
                && principal.permits_model(&item.id)
                && routing::model_eligible(item, &constraints)
            })
          })
        })
        .map(|group| model(&group.id)),
    );
    models
  }
  pub fn observations(&self) -> Observations {
    self
      .inner
      .signals
      .snapshot(self.inner.permits.available_permits())
  }
  /// Executes the same authenticated HTTP pipeline in process without a listener.
  pub async fn invoke(
    &self,
    request: axum::http::Request<axum::body::Body>,
  ) -> axum::response::Response {
    use tower::ServiceExt;
    match self.router().oneshot(request).await {
      Ok(response) => response,
      Err(never) => match never {},
    }
  }
  pub fn router(&self) -> axum::Router {
    http::router(self.clone())
  }
}
