use agent_app::{
  AgentCommand, RuntimeCommandError, UnsupportedCommand, spawn_runtime_waiting_for_model,
};
use agent_kernel::Kernel;

#[tokio::test]
async fn local_runtime_rejects_daemon_owned_model_download_commands() {
  let (controller, task) = spawn_runtime_waiting_for_model(Kernel::builder().build(), 4);

  assert_eq!(
    controller
      .execute(AgentCommand::StartModelDownload {
        model_id: "qwen3-1.7b-q4_k_m".into(),
      })
      .await,
    Err(RuntimeCommandError::Unsupported(
      UnsupportedCommand::StartModelDownload
    ))
  );
  assert_eq!(
    controller
      .execute(AgentCommand::CancelModelDownload {
        operation_id: "download-1".into(),
      })
      .await,
    Err(RuntimeCommandError::Unsupported(
      UnsupportedCommand::CancelModelDownload
    ))
  );

  drop(controller);
  task.join().await.unwrap();
}
