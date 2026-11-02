use crate::backend::Update;
use agent_client::AgentClient;
use tokio::sync::mpsc;

pub(crate) async fn refresh_session(
  client: &AgentClient,
  session: &str,
  generation: u64,
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
  let _ = updates
    .send(Update::Runs {
      session: session.to_owned(),
      generation,
      runs,
    })
    .await;
  let _ = updates
    .send(Update::Approvals {
      session: session.to_owned(),
      generation,
      approvals,
    })
    .await;
  Ok(active)
}

pub(crate) async fn watch(
  client: &AgentClient,
  run: &str,
  generation: u64,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
  let events = client.watch_run(run, 0).await?;
  let updates = updates.clone();
  let run = run.to_owned();
  Ok(tokio::spawn(deliver(events, run, generation, updates)))
}

async fn deliver(
  mut events: tonic::Streaming<agent_protocol::v1::Event>,
  run_id: String,
  generation: u64,
  updates: mpsc::Sender<Update>,
) {
  loop {
    match events.message().await {
      Ok(Some(event)) => {
        if updates
          .send(Update::Event { generation, event })
          .await
          .is_err()
        {
          break;
        }
      }
      Ok(None) => break,
      Err(error) => {
        tracing::warn!(operation="run.watch",outcome="disconnected",process_id=std::process::id(),run_id,status_code=?error.code(),"Terminal run event stream interrupted");
        let _ = updates
          .send(Update::Notice(format!(
            "Event stream interrupted: {error}. Task state remains in agentd."
          )))
          .await;
        break;
      }
    }
  }
}

pub(crate) fn watch_session(
  client: AgentClient,
  session: String,
  active: Option<String>,
  generation: u64,
  updates: mpsc::Sender<Update>,
) -> tokio::task::JoinHandle<()> {
  tokio::spawn(async move {
    let history = replay_history(&client, &session, generation, &updates);
    let live = async {
      if let Some(run) = active
        && let Ok(events) = client.watch_run(&run, 0).await
      {
        deliver(events, run, generation, updates.clone()).await;
      }
    };
    let (history_result, ()) = tokio::join!(history, live);
    if history_result.is_err() {
      let _ = updates
        .send(Update::Notice(
          "执行记录回放中断；重新选择会话可重新读取，不会重放执行。".into(),
        ))
        .await;
    }
  })
}

/// Replay terminal runs through the same projection as live delivery. Never starts a run.
pub(crate) async fn replay_history(
  client: &AgentClient,
  session: &str,
  generation: u64,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<()> {
  let runs = client.list_runs(session).await?;
  for run in runs.into_iter().filter(|run| {
    matches!(
      run.status.as_str(),
      "completed" | "failed" | "cancelled" | "uncertain"
    )
  }) {
    let mut events = client.watch_run(&run.id, 0).await?;
    while let Some(event) = events.message().await? {
      if updates
        .send(Update::Event { generation, event })
        .await
        .is_err()
      {
        return Ok(());
      }
    }
  }
  Ok(())
}
