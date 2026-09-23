use std::path::PathBuf;

#[tokio::test]
async fn sessions_command_lists_persisted_session_ids() {
  let root = std::env::temp_dir().join(format!(
    "lyonbrown-cli-storage-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_nanos()
  ));
  std::fs::create_dir_all(&root).unwrap();
  let db_path = root.join("agent.db");
  let config_path: PathBuf = root.join("config.toml");
  std::fs::write(
    &config_path,
    format!(
      "[storage]\npath = '{}'\n",
      db_path.display().to_string().replace('\\', "/")
    ),
  )
  .unwrap();
  let store = agent_storage::SessionStore::open(&db_path).await.unwrap();
  let id = agent_core::SessionId::new();
  let writer = store.writer(id);
  writer
    .record(agent_core::AgentEvent::SessionStarted {
      session_id: id,
      ts: chrono::Utc::now(),
    })
    .unwrap();
  writer.close().await.unwrap();
  drop(store);

  let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_agent"))
    .arg("--config")
    .arg(&config_path)
    .arg("sessions")
    .output()
    .await
    .unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  assert!(String::from_utf8_lossy(&output.stdout).contains(&id.to_string()));
  std::fs::remove_dir_all(root).unwrap();
}
