use std::net::IpAddr;
use std::time::Duration;

use agent_app::spawn_runtime;
use agent_client::{AgentClient, AuthenticatedTarget};
use agent_kernel::Kernel;
use agent_local::{DiscoveryDescriptor, ScopeFingerprint, SecretToken};
use agent_runtime::config::{LoadOptions, RuntimeConfig};
use agentd::runtime::{
  DaemonPaths, bind_loopback, load_runtime_config, serve_runtime_until,
  validate_runtime_fingerprint,
};
use clap::Parser as _;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[test]
fn daemon_paths_are_scoped_by_workspace_and_runtime_config() {
  let temporary = tempfile::tempdir().expect("temporary root");
  let home = temporary.path().join("home");
  let workspace = temporary.path().join("workspace");
  std::fs::create_dir_all(&home).expect("home directory");
  std::fs::create_dir_all(&workspace).expect("workspace directory");

  let first = DaemonPaths::resolve(&workspace, &home, "runtime-v1").expect("first paths");
  let same = DaemonPaths::resolve(&workspace, &home, "runtime-v1").expect("same paths");
  let changed = DaemonPaths::resolve(&workspace, &home, "runtime-v2").expect("changed paths");

  assert_eq!(first.scope_fingerprint(), same.scope_fingerprint());
  assert_ne!(first.scope_fingerprint(), changed.scope_fingerprint());
  assert_eq!(first.run_directory(), same.run_directory());
  assert_ne!(first.run_directory(), changed.run_directory());
  assert_eq!(
    first.run_directory().file_name().unwrap(),
    first.scope_fingerprint().as_str()
  );
  assert_eq!(first.workspace(), workspace.canonicalize().unwrap());
  assert!(first.run_directory().starts_with(&home));
}

#[test]
fn effective_runtime_configuration_must_match_the_client_fingerprint() {
  let temporary = tempfile::tempdir().expect("temporary root");
  let mut options = LoadOptions::for_workspace(temporary.path());
  options.global_config = temporary.path().join("missing.toml");
  options.environment = Some(Default::default());
  let config = RuntimeConfig::load(options).expect("default config");
  let fingerprint = config.fingerprint().expect("fingerprint");

  validate_runtime_fingerprint(&config, &fingerprint).expect("matching fingerprint");
  let error = validate_runtime_fingerprint(&config, &"00".repeat(32))
    .expect_err("mismatched fingerprint must fail closed");
  assert!(error.to_string().contains("does not match"));
}

#[test]
fn runtime_configuration_receives_spawn_overrides() {
  let temporary = tempfile::tempdir().expect("temporary root");
  let home = temporary.path().join("home");
  let workspace = temporary.path().join("workspace");
  std::fs::create_dir_all(&home).unwrap();
  std::fs::create_dir_all(&workspace).unwrap();
  let paths = DaemonPaths::resolve(&workspace, &home, "runtime-v1").unwrap();
  let args = agentd::args::Args::try_parse_from([
    "agentd",
    "--workspace",
    workspace.to_str().unwrap(),
    "--runtime-config-fingerprint",
    "runtime-v1",
    "--profile",
    "default-dev",
    "--provider",
    "ollama",
    "--model",
    "qwen3:8b",
    "--endpoint",
    "http://127.0.0.1:22434",
    "--deny-all",
  ])
  .unwrap();

  let config = load_runtime_config(&args, &paths).expect("runtime config");

  assert_eq!(config.profile, "default-dev");
  assert_eq!(config.model.provider, "ollama");
  assert_eq!(config.model.model.as_deref(), Some("qwen3:8b"));
  assert_eq!(config.ollama.endpoint, "http://127.0.0.1:22434");
  assert!(config.approval.deny_all);
}

#[tokio::test]
async fn listener_is_allocated_on_loopback_only() {
  let listener = bind_loopback().await.expect("loopback listener");
  let address = listener.local_addr().expect("listener address");

  assert!(matches!(address.ip(), IpAddr::V4(ip) if ip.is_loopback()));
  assert_ne!(address.port(), 0);
}

#[tokio::test]
async fn shutdown_stops_rpc_before_joining_the_runtime() {
  let listener = bind_loopback().await.expect("loopback listener");
  let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
  let token = SecretToken::generate().expect("token");
  let server_instance_id = Uuid::new_v4();
  let scope = ScopeFingerprint::parse(&"ab".repeat(32)).expect("scope");

  tokio::time::timeout(
    Duration::from_secs(2),
    serve_runtime_until(
      listener,
      controller,
      runtime,
      token,
      server_instance_id,
      scope,
      async {},
    ),
  )
  .await
  .expect("lifecycle must stop")
  .expect("clean lifecycle");
}

#[tokio::test]
async fn runtime_failure_stops_the_rpc_server_without_waiting_for_shutdown() {
  let listener = bind_loopback().await.expect("loopback listener");
  let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
  runtime.abort();
  let token = SecretToken::generate().expect("token");
  let server_instance_id = Uuid::new_v4();
  let scope = ScopeFingerprint::parse(&"ef".repeat(32)).expect("scope");

  let result = tokio::time::timeout(
    Duration::from_secs(2),
    serve_runtime_until(
      listener,
      controller,
      runtime,
      token,
      server_instance_id,
      scope,
      std::future::pending(),
    ),
  )
  .await
  .expect("runtime exit must stop the server");

  assert!(result.unwrap_err().to_string().contains("runtime"));
}

#[tokio::test]
async fn authenticated_handshake_reports_the_published_identity() {
  let listener = bind_loopback().await.expect("loopback listener");
  let address = listener.local_addr().unwrap();
  let (controller, runtime) = spawn_runtime(Kernel::builder().build(), 4);
  let token = SecretToken::generate().expect("token");
  let server_instance_id = Uuid::new_v4();
  let scope = ScopeFingerprint::parse(&"cd".repeat(32)).expect("scope");
  let descriptor = DiscoveryDescriptor::new(
    address,
    std::process::id(),
    server_instance_id,
    token.clone(),
    scope.clone(),
  )
  .expect("descriptor");
  let target = AuthenticatedTarget::from_descriptor(&descriptor);
  let shutdown = CancellationToken::new();
  let server_shutdown = shutdown.clone();
  let expected_scope = scope.clone();
  let server = tokio::spawn(serve_runtime_until(
    listener,
    controller,
    runtime,
    token,
    server_instance_id,
    scope,
    async move { server_shutdown.cancelled().await },
  ));

  let mut client = AgentClient::connect(target, "runtime-lifecycle")
    .await
    .expect("authenticated client");
  let response = client
    .handshake("runtime-lifecycle", "test")
    .await
    .expect("handshake");
  assert_eq!(response.server_instance_id, server_instance_id.to_string());
  assert_eq!(response.scope_fingerprint, expected_scope.as_str());

  shutdown.cancel();
  server.await.expect("server task").expect("clean shutdown");
}

#[test]
fn secure_run_directory_opens_for_the_current_user() {
  let temporary = tempfile::tempdir().expect("temporary root");
  let run_directory = temporary.path().join("run");
  let opened =
    agentd::runtime::open_secure_run_directory(&run_directory).expect("secure run directory");

  assert_eq!(opened.path(), run_directory.canonicalize().unwrap());
}
