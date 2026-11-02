mod composition;
mod config;
use anyhow::{Context, Result};
use fluxdi::{
  Shared,
  axum::{InjectorState, request_scope},
};

#[tokio::main]
async fn main() -> Result<()> {
  let settings = Shared::new(config::AppConfig::load()?);
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
  let module = composition::GatewayModule::new(settings.clone());
  let application = module.resolve().await?;
  let gateway = application.gateway.clone();
  let origin = format!("http://{}", settings.listen);
  let scoped = InjectorState::new(Shared::new(module.injector().clone()));
  let router = gateway
    .router()
    .merge(gateway_web::router_with_options(
      application.storage.clone(),
      gateway.clone(),
      application.decision.clone(),
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
