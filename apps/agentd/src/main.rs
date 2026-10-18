use agentd::{Daemon, config::Config};
use anyhow::{Context, Result};

#[cfg(windows)]
mod windows_service;

fn main() -> Result<()> {
  #[cfg(windows)]
  if let Some(name) = windows_service::requested_name()? {
    return windows_service::dispatch(&name);
  }
  run_console()
}

#[tokio::main]
async fn run_console() -> Result<()> {
  // Only the development supervisor opts into this private, credential-free readiness pipe.
  let ready_nonce = std::env::var("LYONBROWN_DEV_READY_NONCE").ok();
  if let Some(nonce) = &ready_nonce {
    anyhow::ensure!(
      nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()),
      "invalid development readiness nonce"
    );
  }
  let parent_closed = tokio_util::sync::CancellationToken::new();
  if ready_nonce.is_some() {
    monitor_development_parent(parent_closed.clone())?;
  }
  let shutdown = async {
    tokio::select! {
      _ = shutdown_signal() => {},
      _ = parent_closed.cancelled() => {},
    }
  };
  run(Config::load()?, shutdown, move || {
    if let Some(nonce) = ready_nonce {
      use std::io::Write;
      let mut stdout = std::io::stdout().lock();
      writeln!(stdout, "lyonbrown-dev-ready:{nonce}")?;
      stdout.flush()?;
    }
    Ok(())
  })
  .await
}

fn monitor_development_parent(closed: tokio_util::sync::CancellationToken) -> Result<()> {
  // A private pipe reaches EOF even if the supervisor dies without running Rust destructors.
  // Keep this blocking reader outside Tokio so runtime shutdown never waits for stdin.
  std::thread::Builder::new()
    .name("development-parent".into())
    .spawn(move || {
      use std::io::Read;
      let mut input = std::io::stdin().lock();
      let mut byte = [0_u8; 1];
      loop {
        match input.read(&mut byte) {
          Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
          _ => {
            closed.cancel();
            break;
          }
        }
      }
    })
    .context("monitor development supervisor lifetime")?;
  Ok(())
}

async fn run(
  config: Config,
  shutdown: impl std::future::Future<Output = ()>,
  ready: impl FnOnce() -> Result<()>,
) -> Result<()> {
  if config.compatibility_json {
    println!(
      "{}",
      serde_json::json!({"daemon_version":env!("CARGO_PKG_VERSION"),"protocol_min":1,"protocol_max":1,"storage_schema_min":1,"storage_schema_max":1})
    );
    return Ok(());
  }
  anyhow::ensure!(
    config.listen.ip().is_loopback(),
    "local client endpoint must bind loopback; peer TLS endpoint is configured separately"
  );
  let data = config.data_path()?;
  let log_options = agent_telemetry::LogOptions {
    directory: config
      .log_directory
      .clone()
      .unwrap_or_else(|| data.join("logs")),
    retained_files: config.log_retention,
    stderr: false,
  };
  let _logs = agent_telemetry::init_with_options(
    &log_options,
    "agentd",
    &std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
  )?;
  let result = serve(config, shutdown, ready).await;
  if let Err(error) = &result {
    tracing::error!(
      operation = "daemon.serve",
      outcome = "failed",
      error = %error,
      recovery = "Inspect daemon configuration, store permissions and listener availability",
      "Daemon startup or serving failed"
    );
  }
  result
}

async fn serve(
  config: Config,
  shutdown: impl std::future::Future<Output = ()>,
  ready: impl FnOnce() -> Result<()>,
) -> Result<()> {
  let daemon = Daemon::open(config.clone()).await?;
  if let Some(command) = &config.maintenance {
    agentd::peers::maintain(&daemon, command)?;
    return Ok(());
  }
  let incoming = tonic::transport::server::TcpIncoming::bind(config.listen)?;
  daemon.recover().await?;
  let peer = agentd::peers::PeerRuntime::prepare(daemon.clone()).await?;
  if let Err(error) = ready() {
    daemon.shutdown().await;
    return Err(error);
  }
  tracing::info!(address=%config.listen,node_id=%daemon.node_id,"agentd listening");
  let local = tonic::transport::Server::builder()
    .add_service(agentd::client_service(daemon.clone()))
    .add_service(agentd::workspace_service(daemon.clone()))
    .add_service(agentd::browser_service(daemon.clone()))
    .add_service(agentd::runtime_service(daemon.clone()))
    .add_service(agentd::computer_runtime_service(daemon.clone()))
    .add_service(agentd::orchestration_service(daemon.clone()))
    .serve_with_incoming_shutdown(incoming, async {
      shutdown.await;
      daemon.shutdown().await;
    });
  let remote = async {
    if let Some(peer) = peer {
      peer.serve().await
    } else {
      std::future::pending::<Result<()>>().await
    }
  };
  tokio::pin!(local, remote);
  let result = tokio::select! {
    result = &mut local => result.map_err(anyhow::Error::from),
    result = &mut remote => result,
  };
  daemon.shutdown().await;
  result
}

async fn shutdown_signal() {
  #[cfg(unix)]
  {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
      Ok(mut terminate) => {
        tokio::select! {_=terminate.recv()=>{},_=tokio::signal::ctrl_c()=>{}}
      }
      Err(_) => {
        let _ = tokio::signal::ctrl_c().await;
      }
    }
  }
  #[cfg(not(unix))]
  {
    let _ = tokio::signal::ctrl_c().await;
  }
}
