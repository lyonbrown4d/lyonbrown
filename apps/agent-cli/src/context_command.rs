use crate::resource_command::ResourceCommand;
use agent_client::AgentClient;
use agent_protocol::v1;
pub(crate) fn parse(root: &str, args: &[String]) -> Result<ResourceCommand, String> {
  match (root, args) {
    ("/context", []) => Ok(ResourceCommand::Context),
    ("/context", [arg]) if arg == "summary" => Ok(ResourceCommand::ContextSummary),
    ("/context", [arg]) if arg == "undo" => Ok(ResourceCommand::ContextUndo),
    ("/context", [verb, id]) if verb == "cancel" => {
      uuid::Uuid::parse_str(id).map_err(|_| "Context job ID must be a UUID".to_owned())?;
      Ok(ResourceCommand::ContextCancel { job_id: id.clone() })
    }
    ("/context", [verb, job, request, reason @ ..]) if verb == "abandon" && !reason.is_empty() => {
      uuid::Uuid::parse_str(job).map_err(|_| "Context job ID must be a UUID".to_owned())?;
      uuid::Uuid::parse_str(request).map_err(|_| "Resolution request ID must be a UUID".to_owned())?;
      Ok(ResourceCommand::ContextAbandon { job_id: job.clone(), request_id: request.clone(), reason: reason.join(" ") })
    }
    ("/compact", focus) => Ok(ResourceCommand::ContextCompact {
      focus: focus.join(" "),
    }),
    ("/mode", []) => Ok(ResourceCommand::Mode { mode: None }),
    ("/mode", [mode]) => {
      let mode = match mode.as_str() {
        "read-only" => v1::InteractionMode::ReadOnly,
        "plan" => v1::InteractionMode::Plan,
        "execute" => v1::InteractionMode::Execute,
        _ => return Err("Usage: /mode [read-only|plan|execute]".into()),
      };
      Ok(ResourceCommand::Mode { mode: Some(mode) })
    }
    _ => Err(
      "Usage: /context [summary|undo|cancel JOB_ID] · /compact [focus] · /mode [read-only|plan|execute]".into(),
    ),
  }
}
pub(crate) async fn execute(
  client: &AgentClient,
  session_id: &str,
  command: ResourceCommand,
) -> anyhow::Result<(String, String)> {
  if let ResourceCommand::Mode { mode } = command {
    let mut policy = client.interaction_policy(session_id).await?;
    if let Some(mode) = mode {
      let session = client
        .runtime_status(session_id)
        .await?
        .session
        .ok_or_else(|| anyhow::anyhow!("Session unavailable"))?;
      policy = client
        .set_interaction_mode(session_id, session.owner_epoch, policy.revision, mode)
        .await?;
    }
    let mut text = format!(
      "Mode: {:?} · revision {}\nMode restricts existing authority; approvals still apply.\n",
      v1::InteractionMode::try_from(policy.mode).ok(),
      policy.revision
    );
    for tool in policy.tools {
      text.push_str(&format!(
        "{} · {}{}\n",
        tool.name,
        agent_client::tool_availability_label(&tool),
        if tool.requires_approval {
          " · approval required"
        } else {
          ""
        }
      ));
    }
    return Ok(("Session mode and tools".into(), text));
  }
  if command == ResourceCommand::ContextSummary {
    let summary = client.context_summary(session_id).await?;
    return Ok((
      format!("Summary · revision {}", summary.revision),
      if summary.text.is_empty() {
        "No summary".into()
      } else {
        summary.text
      },
    ));
  }
  if let ResourceCommand::ContextAbandon {
    job_id,
    request_id,
    reason,
  } = &command
  {
    let job = client.context_maintenance(session_id).await?;
    anyhow::ensure!(
      &job.id == job_id,
      "Context job does not match the selected session"
    );
    client
      .abandon_context(&job.id, job.owner_epoch, request_id, reason)
      .await?;
    return Ok(("Uncertain context closed".into(), "Original context and unknown intent remain saved. This session cannot execute again. Use /new to continue; nothing was replayed.".into()));
  }
  if let ResourceCommand::ContextCancel { job_id } = &command {
    let job = client.context_maintenance(session_id).await?;
    anyhow::ensure!(
      &job.id == job_id,
      "Context job does not match the selected session"
    );
    let job = client.cancel_context(&job.id, job.owner_epoch).await?;
    return Ok((
      "Context cancellation requested".into(),
      format!(
        "{} · {:?} · cancel requested {}. Refresh /context before further action.",
        job.id,
        v1::ContextMaintenanceState::try_from(job.state).ok(),
        job.cancel_requested
      ),
    ));
  }
  let mut context = match client.context_status(session_id).await {
    Ok(context) => context,
    Err(error) => {
      if let Ok(job) = client.context_maintenance(session_id).await {
        return Ok((
          "Context maintenance".into(),
          format!(
            "{} · {:?} · owner epoch {}. Context statistics unavailable. Use /context cancel JOB_ID or /context abandon JOB_ID REQUEST_UUID REASON after inspection.",
            job.id,
            v1::ContextMaintenanceState::try_from(job.state).ok(),
            job.owner_epoch
          ),
        ));
      }
      return Err(error);
    }
  };
  if !matches!(
    command,
    ResourceCommand::Context | ResourceCommand::ContextCancel { .. }
  ) {
    let session = client
      .runtime_status(session_id)
      .await?
      .session
      .ok_or_else(|| anyhow::anyhow!("Session unavailable"))?;
    context = match command {
      ResourceCommand::ContextCompact { focus } => {
        client
          .compact_context(session_id, session.owner_epoch, context.revision, &focus)
          .await?
      }
      ResourceCommand::ContextUndo => {
        client
          .undo_context_compaction(session_id, session.owner_epoch, context.revision)
          .await?
      }
      _ => anyhow::bail!("unsupported context command"),
    };
  }
  let job = context.maintenance.as_ref().map_or_else(
    || "none".to_owned(),
    |job| {
      format!(
        "{} · {:?} · cancel requested {}",
        job.id,
        v1::ContextMaintenanceState::try_from(job.state).ok(),
        job.cancel_requested
      )
    },
  );
  Ok((
    "Session context".into(),
    format!(
      "Revision: {}\nMessages: {} · compactions: {}\nEstimated input: {} tokens (heuristic; idle inspection excludes tool schemas) · window: {} · output reserve: {}\nSummary: {} · undo: {} · pending request: {}\nSummary pipeline available: {}\nMaintenance job: {}\n/context cancel JOB_ID\n/context abandon JOB_ID REQUEST_UUID REASON (close an uncertain session)\n/context summary · /compact [focus] · /context undo",
      context.revision,
      context.message_count,
      context.compaction_count,
      context.estimated_input_tokens,
      context.context_window,
      context.reserved_output_tokens,
      context.has_summary,
      context.can_undo,
      context.summary_request_pending,
      context.summary_pipeline_available,
      job
    ),
  ))
}
