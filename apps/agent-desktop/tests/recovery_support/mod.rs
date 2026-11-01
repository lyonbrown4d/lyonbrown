//! Loopback-only recovery scenario shared by component and visible native acceptance.
use crate::{peer_support::Pair, remote_support::Remote};
use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use std::time::Duration;

pub struct Scenario {
  pub remote: Remote,
  pub model: MockModelServer,
  pub worker_model: MockModelServer,
  pub original: v1::Run,
  pub task_id: String,
  pub dispatch: tokio::task::JoinHandle<()>,
}

impl Scenario {
  pub async fn open(command_timeout_ms: u64) -> Self {
    let protocol = StandardProtocol::ChatCompletions;
    let model = MockModelServer::start(vec![
      tool_reply(protocol, "execute_command", serde_json::json!({
        "executable":"/bin/sh", "arguments":["-c", "printf 'effect\\n' >> effect.txt; exec /bin/sleep 600"],
        "cwd":".", "timeout_ms":command_timeout_ms, "output_limit_bytes":4096
      })),
      text_reply(protocol, "已根据人工核对记录完成汇总；没有重复执行原命令。"),
    ]).await.expect("loopback main model");
    let worker_model =
      MockModelServer::start(vec![text_reply(protocol, "Worker synthetic result")])
        .await
        .expect("loopback worker model");
    let pair = Pair::open_configured(&model, &worker_model, protocol, |config, _| {
      config.commands.executables.insert("/bin/sh".into());
      config.run_timeout_secs = 600;
    })
    .await;
    let remote = Remote::from_pair(pair).await;
    let original = remote
      .client
      .start_run(
        &remote.pair.session.id,
        "Execute the approved local command once; its file effect must never be replayed.",
        "original-command",
      )
      .await
      .expect("original run");
    tokio::time::timeout(Duration::from_secs(10), async {
      loop {
        if !remote
          .client
          .list_approvals(&original.id)
          .await
          .expect("approvals")
          .is_empty()
        {
          break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .expect("command approval");
    let task_id = agent_core::new_id();
    let task_client = remote.client.clone();
    let target = remote.target();
    let effect = std::path::Path::new(&remote.pair.session.workspace).join("effect.txt");
    let envelope = v1::TaskEnvelope {
      task_id: task_id.clone(),
      parent_run_id: original.id.clone(),
      session_id: remote.pair.session.id.clone(),
      owner_node_id: remote.pair.master.node_id.clone(),
      owner_epoch: remote.pair.session.owner_epoch,
      attempt_id: agent_core::new_id(),
      instructions: "Return a synthetic result for explicit Master review.".into(),
      capabilities: vec!["workspace.read".into()],
      workspace: remote.pair.session.workspace.clone(),
      idempotency_key: "recovery-worker".into(),
      deadline_ms: agent_core::now_ms() + 3_600_000,
    };
    // Wait for the approved command's actual effect. An awaiting-approval Run
    // cannot dispatch Tasks; never weaken that production guard for a fixture.
    let dispatch = tokio::spawn(async move {
      tokio::time::timeout(Duration::from_secs(600), async {
        while !effect.exists() {
          tokio::time::sleep(Duration::from_millis(10)).await;
        }
      })
      .await
      .expect("approved command effect");
      let task = task_client
        .dispatch_task(target.clone(), envelope)
        .await
        .expect("paired Worker dispatch");
      let id = task.envelope.expect("task identity").task_id;
      tokio::time::timeout(Duration::from_secs(10), async {
        loop {
          let task = task_client
            .reconcile_task(target.clone(), &id)
            .await
            .expect("Worker reconciliation");
          if task.attempt_status == "succeeded" {
            assert_eq!(task.status, "awaiting_acceptance");
            break;
          }
          tokio::time::sleep(Duration::from_millis(10)).await;
        }
      })
      .await
      .expect("Worker result");
    });
    Self {
      remote,
      model,
      worker_model,
      original,
      task_id,
      dispatch,
    }
  }

  pub fn effect(&self) -> std::path::PathBuf {
    std::path::Path::new(&self.remote.pair.session.workspace).join("effect.txt")
  }

  pub async fn evidence(&self) -> anyhow::Result<serde_json::Value> {
    let store = &self.remote.pair.master.store;
    let resolution = store.run_recovery(&self.original.id)?;
    let fresh = resolution
      .as_ref()
      .and_then(|value| value.replacement_run_id.as_ref())
      .map(|id| store.run(id))
      .transpose()?
      .flatten();
    let requests = self.model.requests().await;
    let approvals = store.approvals(&self.original.id)?;
    let mut command_receipts = Vec::new();
    for approval in &approvals {
      let exact: serde_json::Value = serde_json::from_str(&approval.arguments_json)?;
      if let Some(id) = exact["action_id"].as_str()
        && let Some(receipt) = store.operation(id)?
      {
        command_receipts.push(receipt);
      }
    }
    Ok(serde_json::json!({
      "platform":std::env::consts::OS, "arch":std::env::consts::ARCH, "version":env!("CARGO_PKG_VERSION"),
      "original_run":store.run(&self.original.id)?, "resolution":resolution, "replacement_run":fresh,
      "approvals":approvals, "command_receipts":command_receipts,
      "master_task":store.task(&self.task_id)?, "worker_task":self.remote.pair.worker.store.task(&self.task_id)?,
      "effect":std::fs::read_to_string(self.effect()).ok(), "main_model_requests":requests.len(),
      "worker_model_requests":self.worker_model.requests().await.len(),
      "worker_dispatch_finished":self.dispatch.is_finished(),
      "fresh_request_has_old_command":requests.get(1).is_some_and(|request| request.body.to_string().contains("exec /bin/sleep")),
      "original_events":store.events(&self.original.id, 0, 1000)?,
      "pending_outbox":store.pending_outbox(1000)?
    }))
  }
}
