use agent_client::AgentClient;
use agent_protocol::v1;
use anyhow::{Result, ensure};
use serde_json::json;

use super::output::Output;

pub(super) fn terminal(status: &str) -> bool {
  matches!(status, "completed" | "failed" | "cancelled" | "uncertain")
}

pub(super) fn exit_code(status: &str) -> Option<u8> {
  match status {
    "completed" => Some(0),
    "failed" => Some(1),
    "awaiting_approval" => Some(3),
    "uncertain" => Some(4),
    "cancelled" => Some(130),
    _ => None,
  }
}

pub(super) async fn watch(client: &AgentClient, id: &str, output: &mut Output) -> Result<v1::Run> {
  let mut stream = client.watch_run(id, 0).await?;
  let mut cursor = 0;
  let mut final_run = None;
  let mut poll = tokio::time::interval(std::time::Duration::from_millis(250));
  loop {
    tokio::select! {
      event = stream.message() => {
        let Some(event) = event? else {
          if let Some(run) = final_run { return Ok(run); }
          let run = client.get_run(id).await?;
          ensure!(terminal(&run.status), "daemon stream closed before terminal state");
          // Status can become terminal between the daemon's last batch read
          // and its end-of-stream check. Replay only the remaining durable
          // tail after confirmation; this never resubmits the Run.
          stream = client.watch_run(id, cursor).await?;
          final_run = Some(run);
          continue;
        };
        ensure!(event.run_id == id && event.cursor > cursor, "invalid event identity or cursor");
        cursor = event.cursor;
        let payload = serde_json::from_str::<serde_json::Value>(&event.payload_json)
          .unwrap_or_else(|_| json!(event.payload_json));
        output.record(json!({"type":"event", "run_id":event.run_id,
          "cursor":event.cursor, "kind":event.kind, "payload":payload,
          "created_at_ms":event.created_at_ms})).await?;
      }
      _ = poll.tick(), if final_run.is_none() => {
        let run = client.get_run(id).await?;
        if run.status == "awaiting_approval" { return Ok(run); }
      }
    }
  }
}
