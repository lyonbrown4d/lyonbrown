use crate::resource_command::ResourceCommand;

pub(crate) fn parse(args: &[String]) -> Result<ResourceCommand, String> {
  match args {
    [] => Ok(ResourceCommand::Models),
    [verb] if verb == "list" => Ok(ResourceCommand::Models),
    [verb, model, execution] if verb == "select" => Ok(ResourceCommand::ModelSelect {
      model: model.clone(),
      execution: execution.clone(),
    }),
    [verb, query] if verb == "search" => Ok(hub("search", query, "", "", "", "")),
    [verb, repo, revision] if verb == "inspect" => Ok(hub("inspect", repo, revision, "", "", "")),
    [verb, repo, revision, file, runtime] if verb == "plan" => Ok(hub("plan", repo, revision, file, runtime, "")),
    [verb, repo, revision, file, runtime, id] if verb == "install" => Ok(hub("install", repo, revision, file, runtime, id)),
    [verb] if verb == "installed" => Ok(hub("installed", "", "", "", "", "")),
    [verb, id] if matches!(verb.as_str(), "remove" | "status" | "cancel" | "load" | "unload") => Ok(hub(verb, "", "", "", "", id)),
    _ => Err("Usage: /models list · select MODEL EXECUTION · search QUERY · inspect REPO REVISION · plan REPO REVISION FILE RUNTIME · install REPO REVISION FILE RUNTIME PLAN_ID · installed · status ID · cancel ID · load ID · unload ID · remove ID".into()),
  }
}

fn hub(
  action: &str,
  repository: &str,
  revision: &str,
  file: &str,
  runtime: &str,
  id: &str,
) -> ResourceCommand {
  ResourceCommand::ModelHub {
    action: action.into(),
    repository: repository.into(),
    revision: revision.into(),
    file: file.into(),
    runtime: runtime.into(),
    id: id.into(),
  }
}

pub(crate) fn catalog(models: &[agent_protocol::v1::ModelCandidate]) -> String {
  let mut lines = vec!["Models · /models select MODEL EXECUTION".into()];
  for model in models {
    lines.push(format!(
      "{}{} · {} · {} · {}\n  model: {}",
      if model.selected { "› " } else { "  " },
      model.display_name,
      model.vendor,
      model.source,
      model.state,
      model.id
    ));
    for execution in &model.executions {
      lines.push(format!(
        "    {} · {} · {} · {}{}",
        execution.id,
        execution.label,
        execution.transport,
        execution.state,
        if execution.reason.is_empty() {
          String::new()
        } else {
          format!(" · {}", execution.reason)
        }
      ));
    }
  }
  if models.is_empty() {
    lines.push("No models returned by agentd".into());
  }
  lines.join("\n")
}
