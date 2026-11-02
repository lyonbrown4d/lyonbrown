mod follow;
mod output;

use std::time::Duration;

use agent_client::AgentClient;
use agent_protocol::v1;
use anyhow::{Context, Result};
use serde_json::json;

use crate::{config::ResolvedConfig, exec_command::ExecArgs};
use output::Output;

struct Invocation {
  phase: &'static str,
  key: String,
  session_id: Option<String>,
  client: Option<AgentClient>,
  run: Option<v1::Run>,
}

/// Exec owns a runtime specifically to bound shutdown of blocking launcher or
/// filesystem work after cancellation. TUI retains its normal Tokio entry.
pub(crate) fn run(config: &ResolvedConfig, args: ExecArgs) -> Result<u8> {
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()?;
  let code = runtime.block_on(execute(config, args));
  runtime.shutdown_timeout(Duration::from_millis(100));
  Ok(code)
}

async fn execute(config: &ResolvedConfig, args: ExecArgs) -> u8 {
  let started = std::time::Instant::now();
  tracing::info!(
    operation = "run.exec",
    outcome = "started",
    timeout_secs = args.timeout,
    "Non-interactive execution started"
  );
  let mut output = match Output::new(args.output_format) {
    Ok(output) => output,
    Err(error) => {
      tracing::warn!(
        operation = "run.exec",
        outcome = "stdout_unavailable",
        exit_code = 1,
        "Non-interactive execution cannot write stdout"
      );
      output::diagnostic(&format!("stdout unavailable: {error}"));
      return 1;
    }
  };
  let mut state = Invocation {
    phase: "input",
    key: args
      .idempotency_key
      .clone()
      .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
    session_id: args.session.clone(),
    client: None,
    run: None,
  };
  let result = {
    let execute = submit_and_follow(config, &args, &mut state, &mut output);
    tokio::pin!(execute);
    tokio::select! {
      result = &mut execute => Ok(result),
      _ = crate::terminal::shutdown() => Err(("interrupted", 130)),
      _ = tokio::time::sleep(Duration::from_secs(args.timeout)) => Err(("timeout", 124)),
    }
  };
  let code = match result {
    Err((reason, code)) => stop(&state, &mut output, reason, code).await,
    Ok(Ok(code)) => code,
    Ok(Err(error)) => {
      let code = if state.phase == "input" { 2 } else { 1 };
      let message = format!("{error:#}");
      let report = output.record(json!({
        "type":"error", "phase":state.phase, "message":message,
        "session_id":state.session_id, "run_id":state.run.as_ref().map(|run| &run.id),
        "idempotency_key":state.key,
        "submission_outcome_unknown":state.phase == "submission" && state.run.is_none(),
        "exit_code":code
      }));
      let _ = tokio::time::timeout(Duration::from_secs(1), report).await;
      output::diagnostic(&format!(
        "{message}. No submission was replayed; inspect the recorded session/run before retrying."
      ));
      code
    }
  };
  tracing::info!(
    operation = "run.exec",
    outcome = "finished",
    exit_code = code,
    elapsed_ms = started.elapsed().as_millis() as u64,
    session_id = state.session_id.as_deref().unwrap_or(""),
    run_id = state.run.as_ref().map(|run| run.id.as_str()).unwrap_or(""),
    phase = state.phase,
    "Non-interactive execution finished"
  );
  code
}

async fn submit_and_follow(
  config: &ResolvedConfig,
  args: &ExecArgs,
  state: &mut Invocation,
  output: &mut Output,
) -> Result<u8> {
  let prompt = crate::exec_command::read_prompt(args.prompt.as_deref()).await?;
  state.phase = "connect";
  let client = crate::bootstrap::connect_client(config).await?;
  state.client = Some(client.clone());
  state.phase = "session";
  let session = if let Some(id) = &args.session {
    client
      .runtime_status(id)
      .await?
      .session
      .context("session not found")?
  } else {
    let path = args
      .workspace
      .clone()
      .map(Ok)
      .unwrap_or_else(std::env::current_dir)?;
    let path = if path.is_absolute() {
      path
    } else {
      std::env::current_dir()?.join(path)
    };
    client
      .create_session(
        "CLI exec",
        path.to_str().context("workspace must be UTF-8")?,
      )
      .await?
  };
  state.session_id = Some(session.id.clone());
  output
    .record(json!({"type":"submission", "session_id":session.id,
    "owner_epoch":session.owner_epoch, "idempotency_key":state.key}))
    .await?;
  output::diagnostic(&format!(
    "Session {} · submission {}",
    session.id, state.key
  ));
  state.phase = "submission";
  let run = client
    .start_run_owned(&session.id, session.owner_epoch, &prompt, &state.key)
    .await?;
  state.run = Some(run.clone());
  state.phase = "follow";
  output
    .record(json!({"type":"accepted", "session_id":run.session_id, "run_id":run.id}))
    .await?;
  output::diagnostic(&format!("Run {}", run.id));
  let result = follow::watch(&client, &run.id, output).await?;
  let code =
    follow::exit_code(&result.status).context("event stream ended before a final state")?;
  output.finish(&result, code, None).await?;
  if code != 0 {
    output::diagnostic(&format!(
      "Run {}: {}. {}",
      result.id, result.status, result.error
    ));
  }
  Ok(code)
}

async fn stop(state: &Invocation, output: &mut Output, reason: &str, code: u8) -> u8 {
  let confirmation = async {
    let client = state.client.as_ref().context("no connected daemon")?;
    let run = state.run.as_ref().context("submission not acknowledged")?;
    let mut latest = client.cancel_run(&run.id).await?;
    while !follow::terminal(&latest.status) {
      tokio::time::sleep(Duration::from_millis(50)).await;
      latest = client.get_run(&run.id).await?;
    }
    anyhow::Ok(latest)
  };
  // Cancel is an explicit daemon request, not a local process kill or a claim
  // that tool effects were rolled back. Unknown responses never trigger replay.
  let confirmed = tokio::time::timeout(Duration::from_secs(5), confirmation).await;
  let report = async {
    match confirmed {
      Ok(Ok(run)) => {
        let _ = output.finish(&run, code, Some(reason)).await;
      }
      _ => {
        let _ = output
          .record(json!({"type":"stopped", "reason":reason, "exit_code":code,
        "session_id":state.session_id, "run_id":state.run.as_ref().map(|run| &run.id),
        "idempotency_key":state.key, "cancellation_confirmed":false,
        "submission_outcome_unknown":state.phase == "submission" && state.run.is_none()}))
          .await;
      }
    }
  };
  let _ = tokio::time::timeout(Duration::from_secs(1), report).await;
  output::diagnostic(&format!(
    "exec {reason}; inspect daemon state before retrying. Cancellation does not undo effects."
  ));
  code
}
