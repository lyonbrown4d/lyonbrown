mod composition;
mod config;
use anyhow::{Context, Result};
use decision_core::{DecisionEngine, Policy};
use fluxdi::{
  Shared,
  axum::{InjectorState, request_scope},
};
use gateway_storage::GatewayStorage;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
  let settings = config::AppConfig::load()?;
  let _logs = agent_telemetry::init(&settings.data_dir, "ai-gateway", "info")?;
  settings.validate_web_assets()?;
  let web_options = gateway_web::LeptosOptions::builder()
    .output_name("gateway_web")
    .site_pkg_dir("ui/pkg")
    .site_root(
      settings
        .web_root
        .to_str()
        .context("gateway WebUI directory must be UTF-8")?,
    )
    .site_addr(settings.listen)
    .build();
  let store = Arc::new(GatewayStorage::open(
    settings.data_dir.join("gateway.sqlite"),
  )?);
  store
    .bootstrap_super_admin(
      &settings.admin_username,
      settings.admin_password.expose_secret(),
    )
    .await?;
  store
    .bootstrap_access_token(settings.gateway.token.expose_secret())
    .await?;
  let registry: Vec<_> = settings
    .gateway
    .models
    .iter()
    .map(|model| {
      serde_json::json!({
        "id":model.id,"upstream_model":model.upstream_model,"api_base":model.api_base,
        "tier":model.tier,"local_inference":model.local_inference,"capabilities":model.capabilities,
        "context_tokens":model.context_tokens,
      })
    })
    .collect();
  let revision = hex::encode(Sha256::digest(serde_json::to_vec(&registry)?));
  store
    .seed_models(&revision, &settings.gateway.models)
    .await?;
  let policy = if let Some(policy) = store.load_policy().await? {
    policy
  } else {
    let policy = Policy::default();
    store.save_policy(policy.clone(), None).await?;
    policy
  };
  let decision = Arc::new(DecisionEngine::new(policy, Vec::new())?);
  let module = composition::GatewayModule::new(
    Shared::new(settings.gateway),
    decision.clone(),
    store.clone(),
  );
  let gateway = module.gateway()?;
  let origin = format!("http://{}", settings.listen);
  let scoped = InjectorState::new(Shared::new(module.injector().clone()));
  let router = gateway
    .router()
    .merge(gateway_web::router_with_options(
      store,
      gateway.clone(),
      decision,
      origin,
      web_options,
    ))
    .layer(axum::middleware::from_fn_with_state(scoped, request_scope));
  let listener = tokio::net::TcpListener::bind(settings.listen)
    .await
    .context("bind configured gateway listener")?;
  tracing::info!(address=%listener.local_addr()?, event="gateway_ready", "AI Gateway ready");
  let drain = gateway.clone();
  axum::serve(listener, router)
    .with_graceful_shutdown(async move {
      let _ = tokio::signal::ctrl_c().await;
      drain.set_ready(false);
    })
    .await
    .context("serve AI Gateway")?;
  Ok(())
}
