mod composition;
mod config;

use agent_distributed::{
  identity::{PeerIdentity, PublicIdentity},
  peers::PeerManager,
};
use agent_protocol::v1::{Node, control_service_server::ControlServiceServer};
use agent_storage::Store;
use anyhow::{Result, ensure};
use clap::Parser;
use fluxdi::Shared;

#[tokio::main]
async fn main() -> Result<()> {
  let arguments = config::Arguments::parse();
  let data_dir = match arguments.data_dir.clone() {
    Some(path) => path,
    None => agent_config::default_data_dir()?.join("control"),
  };
  let data_dir = std::path::absolute(data_dir)?;
  std::fs::create_dir_all(&data_dir)?;
  let settings = config::load(&data_dir, &arguments)?;
  let _log = agent_telemetry::init_with_options(
    &agent_telemetry::LogOptions {
      directory: settings.log_directory.clone(),
      retained_files: settings.log_retention,
      stderr: true,
    },
    "agent-control",
    &settings.log_filter,
  )?;
  let identity = PeerIdentity::load_or_create(&data_dir.join("identity"), &settings.node_id)?;
  let store = Store::open(data_dir.join("control.sqlite3"))?;
  let manager = PeerManager::new(
    store.clone(),
    identity.clone(),
    Node {
      id: settings.node_id.clone(),
      name: "Lyonbrown Control".into(),
      protocol_version: 1,
      protocol_min: 1,
      protocol_max: 1,
      ..Default::default()
    },
  )?;
  match arguments.command {
    config::Command::Identity => {
      println!("{}", serde_json::to_string_pretty(&identity.public)?);
      eprintln!(
        "pairing_fingerprint={}",
        identity.public.pairing_fingerprint()?
      );
    }
    config::Command::Pair {
      bundle,
      pairing_fingerprint,
    } => {
      let public: PublicIdentity = serde_json::from_slice(&std::fs::read(bundle)?)?;
      public.validate()?;
      ensure!(
        public.pairing_fingerprint()? == pairing_fingerprint,
        "human-verified pairing fingerprint does not match TLS/Noise identity binding"
      );
      manager.pair_local(
        public,
        ["control.register", "control.resolve", "control.relay"]
          .map(String::from)
          .to_vec(),
      )?;
      println!("Peer paired; restart the control listener to reload its mTLS trust roots.");
    }
    config::Command::Revoke { node_id } => {
      manager.revoke_local(&node_id)?;
    }
    config::Command::Allow { source, target } => {
      manager.trust(&source)?;
      manager.trust(&target)?;
      store.control_allow_link(&source, &target, true)?;
      tracing::info!(operation = "control.policy", outcome = "allowed", source = %source, target = %target, "operator authorized a directional relay link");
    }
    config::Command::Deny { source, target } => {
      store.control_allow_link(&source, &target, false)?;
      tracing::info!(operation = "control.policy", outcome = "denied", source = %source, target = %target, "operator removed a directional relay link");
    }
    config::Command::Serve => {
      if let Some(links) = &settings.allowed_links {
        for link in links {
          manager.trust(&link.source)?;
          manager.trust(&link.target)?;
        }
        store.control_replace_links(
          &links
            .iter()
            .map(|link| (link.source.clone(), link.target.clone()))
            .collect::<Vec<_>>(),
        )?;
      }
      let tls = manager.server_tls()?;
      let module = composition::ControlModule::new(
        Shared::new(manager),
        Shared::new(store),
        Shared::new(settings.clone()),
      );
      let control = (*module.control()?).clone();
      let shutdown_control = control.clone();
      let listener = tokio::net::TcpListener::bind(settings.listen).await?;
      tracing::info!(operation = "control.listen", outcome = "ready", address = %settings.listen, "mTLS control listener ready");
      let (stop, wait) = tokio::sync::oneshot::channel::<()>();
      let server = tonic::transport::Server::builder()
        .tls_config(tls)?
        .add_service(ControlServiceServer::new(control))
        .serve_with_incoming_shutdown(
          tokio_stream::wrappers::TcpListenerStream::new(listener),
          async {
            let _ = wait.await;
          },
        );
      tokio::pin!(server);
      tokio::select! {
        result = &mut server => result?,
        signal = tokio::signal::ctrl_c() => {
          signal?;
          shutdown_control.shutdown();
          let _ = stop.send(());
          match tokio::time::timeout(std::time::Duration::from_secs(5), &mut server).await {
            Ok(result) => result?,
            Err(_) => tracing::warn!(operation = "control.shutdown", outcome = "deadline", "relay drain deadline reached; delivery results may be unknown"),
          }
        }
      }
    }
  }
  Ok(())
}
