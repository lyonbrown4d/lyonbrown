use crate::backend::Update;
use agent_client::AgentClient;
use tokio::sync::mpsc;

pub(crate) async fn refresh_session(
  client: &AgentClient,
  session: &str,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<Option<String>> {
  let runs = client.list_runs(session).await?;
  let active = runs
    .iter()
    .rev()
    .find(|run| {
      matches!(
        run.status.as_str(),
        "accepted" | "running" | "awaiting_approval"
      )
    })
    .map(|run| run.id.clone());
  let batches = futures::future::try_join_all(
    runs
      .iter()
      .filter(|run| run.status == "awaiting_approval")
      .map(|run| client.list_approvals(&run.id)),
  )
  .await?;
  let approvals = batches
    .into_iter()
    .flatten()
    .filter(|approval| approval.status == "pending")
    .collect();
  let _ = updates.send(Update::Runs(runs)).await;
  let _ = updates
    .send(Update::Approvals {
      session: session.to_owned(),
      approvals,
    })
    .await;
  Ok(active)
}

pub(crate) async fn watch(
  client: &AgentClient,
  run: &str,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
  let mut events = client.watch_run(run, 0).await?;
  let updates = updates.clone();
  let run_id = run.to_owned();
  Ok(tokio::spawn(async move {
    loop {
      match events.message().await {
        Ok(Some(event)) => {
          if updates.send(Update::Event(event)).await.is_err() {
            break;
          }
        }
        Ok(None) => break,
        Err(error) => {
          tracing::warn!(
            operation = "run.watch",
            outcome = "disconnected",
            process_id = std::process::id(),
            run_id,
            status_code = ?error.code(),
            "Terminal run event stream interrupted"
          );
          let _ = updates
            .send(Update::Notice(format!(
              "Event stream interrupted: {error}. Task state remains in agentd."
            )))
            .await;
          break;
        }
      }
    }
  }))
}
