use agent_client::AgentClient;

use crate::resource_command::ResourceCommand;

pub(crate) async fn execute(
  client: &AgentClient,
  session: Option<&str>,
  command: ResourceCommand,
) -> anyhow::Result<(String, String)> {
  match command {
    ResourceCommand::Status => {
      let session = required_session(session)?;
      let status = client.runtime_status(session).await?;
      Ok((
        "Runtime status".into(),
        crate::resource_command::format_runtime_status(&status),
      ))
    }
    ResourceCommand::Providers { client_candidates } => {
      let providers = client.list_providers().await?;
      Ok((
        "Providers".into(),
        crate::resource_command::format_providers(&providers, &client_candidates),
      ))
    }
    ResourceCommand::Models => {
      let models = client.list_models(session.unwrap_or_default()).await?;
      Ok(("Models".into(), crate::model_command::catalog(&models)))
    }
    ResourceCommand::ModelSelect { model, execution } => {
      let session_id = required_session(session)?;
      let session = client
        .list_sessions()
        .await?
        .into_iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| anyhow::anyhow!("Session is no longer available"))?;
      client
        .select_model(session_id, session.owner_epoch, &model, &execution)
        .await?;
      let models = client.list_models(session_id).await?;
      Ok((
        "Session model selected".into(),
        crate::model_command::catalog(&models),
      ))
    }
    ResourceCommand::ModelHub {
      action,
      repository,
      revision,
      file,
      runtime,
      id,
    } => {
      let result = client
        .hub_operation(agent_protocol::v1::HubRequest {
          action: action.clone(),
          repository,
          revision,
          file,
          runtime,
          id,
        })
        .await?;
      // Hub replies arrive as native JSON; the client formats them for reading.
      Ok((
        format!("Models · {action}"),
        crate::model_hub_view::format(&action, &result),
      ))
    }
    ResourceCommand::Git => {
      let session = required_session(session)?;
      let status = client.git_status(session).await?;
      Ok((
        "Git workbench".into(),
        crate::resource_command::format_git(&status),
      ))
    }
    ResourceCommand::Diff { path } => {
      let session = required_session(session)?;
      let detail = client.workspace_diff(session, &path).await?;
      Ok((
        if path.is_empty() {
          "Workspace diff".into()
        } else {
          format!("Diff · {path}")
        },
        if detail.is_empty() {
          "No changes".into()
        } else {
          detail
        },
      ))
    }
    ResourceCommand::SkillsList { scope } => {
      let session = required_session(session)?;
      let skills = client.list_skills(&scope, session).await?;
      Ok((
        format!("Skills · {scope}"),
        crate::resource_command::format_skills(&skills),
      ))
    }
    ResourceCommand::SkillInspect { source, scope } => {
      let session = required_session(session)?;
      let inspection = client.inspect_skill(&source, &scope, session).await?;
      Ok((
        format!("Skill inspection · {source}"),
        crate::resource_command::format_skill_inspection(&inspection),
      ))
    }
    ResourceCommand::SkillInstall {
      source,
      scope,
      expected_sha256,
    } => {
      let session = required_session(session)?;
      let skill = client
        .install_skill(&source, &scope, session, &expected_sha256)
        .await?;
      Ok((
        format!("Skill installed · {scope}"),
        crate::resource_command::format_skills(&[skill]),
      ))
    }
    ResourceCommand::SkillRemove {
      name,
      scope,
      expected_sha256,
    } => {
      let session = required_session(session)?;
      let skill = client
        .remove_skill(&name, &scope, session, &expected_sha256)
        .await?;
      Ok((
        format!("Skill removed · {scope}"),
        crate::resource_command::format_skills(&[skill]),
      ))
    }
    ResourceCommand::ProviderUpload {
      provider,
      environment,
    } => {
      let definition = agent_config::provider_definition(&provider)
        .ok_or_else(|| anyhow::anyhow!("Unsupported provider: {provider}"))?;
      anyhow::ensure!(
        definition
          .credential_env_names
          .contains(&environment.as_str()),
        "Environment variable is not allowlisted for provider {}",
        definition.id
      );
      let credential = std::env::var(&environment)
        .map_err(|_| {
          anyhow::anyhow!("Allowlisted environment variable is not available: {environment}")
        })?
        .into_bytes();
      anyhow::ensure!(!credential.is_empty(), "Provider credential is empty");
      let provider = client
        .put_provider_credential(definition.id, credential)
        .await?;
      Ok((
        "Provider credential uploaded".into(),
        format!(
          "{} · credential {} · value hidden",
          provider.name, provider.credential_state
        ),
      ))
    }
    ResourceCommand::ProviderRemove { provider } => {
      let definition = agent_config::provider_definition(&provider)
        .ok_or_else(|| anyhow::anyhow!("Unsupported provider: {provider}"))?;
      let provider = client.remove_provider_credential(definition.id).await?;
      Ok((
        "Provider credential removed".into(),
        format!(
          "{} · credential {}",
          provider.name, provider.credential_state
        ),
      ))
    }
  }
}

fn required_session(session: Option<&str>) -> anyhow::Result<&str> {
  session.ok_or_else(|| anyhow::anyhow!("Select a session first"))
}
