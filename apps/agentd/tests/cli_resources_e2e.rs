//! Authenticated client-to-daemon regression coverage for terminal resources.
use agent_client::AgentClient;
use clap::Parser;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credentials_skills_and_git_cross_the_authenticated_transport() {
  let data = tempfile::tempdir().unwrap();
  let workspace = tempfile::tempdir().unwrap();
  let source = tempfile::tempdir().unwrap();
  let skill = source.path().join("fixture-skill");
  std::fs::create_dir(&skill).unwrap();
  std::fs::write(skill.join("SKILL.md"), "---\nname: fixture-skill\ndescription: Explain fixture files\n---\nRead the workspace carefully.\n").unwrap();
  let mut config = agentd::config::Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(data.path().into());
  let daemon = agentd::Daemon::open(config).await.unwrap();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let endpoint = format!("http://{}", listener.local_addr().unwrap());
  let serving = daemon.clone();
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(agentd::client_service(serving.clone()))
      .add_service(agentd::workspace_service(serving.clone()))
      .add_service(agentd::skill_service(serving))
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
      .unwrap();
  });
  let client = AgentClient::connect(&endpoint, daemon.auth_token())
    .await
    .unwrap();
  let session = client
    .create_session("resource e2e", workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let uploaded = client
    .put_provider_credential("openai", b"synthetic-e2e-credential".to_vec())
    .await
    .unwrap();
  assert_eq!(uploaded.credential_source, "uploaded_encrypted");
  assert!(!format!("{uploaded:?}").contains("synthetic-e2e-credential"));
  let reopened = agent_secrets::SecretStore::open(data.path().join("provider-secrets")).unwrap();
  assert!(
    reopened
      .list()
      .unwrap()
      .iter()
      .any(|entry| entry.id == "provider-openai")
  );
  let source = skill.to_str().unwrap();
  for scope in ["daemon_user", "workspace"] {
    let inspected = client
      .inspect_skill(source, scope, &session.id)
      .await
      .unwrap()
      .skill
      .unwrap();
    assert!(
      client
        .install_skill(source, scope, &session.id, &"0".repeat(64))
        .await
        .is_err()
    );
    assert!(
      client
        .list_skills(scope, &session.id)
        .await
        .unwrap()
        .is_empty()
    );
    let installed = client
      .install_skill(source, scope, &session.id, &inspected.content_sha256)
      .await
      .unwrap();
    assert_eq!(installed.name, "fixture-skill");
    assert_eq!(
      client.list_skills(scope, &session.id).await.unwrap().len(),
      1
    );
    assert!(
      client
        .remove_skill("fixture-skill", scope, &session.id, &"0".repeat(64))
        .await
        .is_err()
    );
    client
      .remove_skill(
        "fixture-skill",
        scope,
        &session.id,
        &installed.content_sha256,
      )
      .await
      .unwrap();
    assert!(
      client
        .list_skills(scope, &session.id)
        .await
        .unwrap()
        .is_empty()
    );
  }
  assert!(
    std::process::Command::new("git")
      .args(["init", "--quiet"])
      .current_dir(workspace.path())
      .status()
      .unwrap()
      .success()
  );
  std::fs::write(workspace.path().join("notes.txt"), "untracked fixture").unwrap();
  let git = client.git_status(&session.id).await.unwrap();
  assert!(git.repository);
  assert!(
    git
      .entries
      .iter()
      .any(|entry| entry.path == "notes.txt" && entry.index_status == "?")
  );
  let wrong = AgentClient::connect(&endpoint, "wrong-token").await;
  if let Ok(wrong) = wrong {
    assert!(wrong.list_providers().await.is_err());
  }
  client.remove_provider_credential("openai").await.unwrap();
  assert!(reopened.list().unwrap().is_empty());
  daemon.shutdown().await;
  server.abort();
}
