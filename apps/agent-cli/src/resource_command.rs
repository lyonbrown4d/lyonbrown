use agent_config::CredentialState;
use agent_protocol::v1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceCommand {
  Status,
  Providers {
    client_candidates: Vec<ClientProviderCandidate>,
  },
  Models,
  Git,
  Diff {
    path: String,
  },
  SkillsList {
    scope: String,
  },
  SkillInspect {
    source: String,
    scope: String,
  },
  SkillInstall {
    source: String,
    scope: String,
    expected_sha256: String,
  },
  SkillRemove {
    name: String,
    scope: String,
    expected_sha256: String,
  },
  ProviderUpload {
    provider: String,
    environment: String,
  },
  ProviderRemove {
    provider: String,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientProviderCandidate {
  pub environment: String,
  pub state: String,
}

pub fn candidate_environment_names() -> Vec<ClientProviderCandidate> {
  agent_config::detect_provider_environment()
    .into_iter()
    .filter(|(_, status)| {
      matches!(
        status.state,
        CredentialState::Detected | CredentialState::Invalid
      )
    })
    .map(|(environment, status)| ClientProviderCandidate {
      environment,
      state: credential_state(&status).into(),
    })
    .collect()
}

fn credential_state(status: &agent_config::ProviderCredentialStatus) -> &'static str {
  match status.state {
    CredentialState::Missing => "missing",
    CredentialState::Detected => "detected",
    CredentialState::Invalid => "invalid",
    CredentialState::NotRequired => "not required",
  }
}

pub fn format_runtime_status(status: &v1::RuntimeStatus) -> String {
  let node = status.node.as_ref();
  let session = status.session.as_ref();
  let run = status.active_run.as_ref();
  format!(
    "Daemon: {} · {}\nSession: {} · {}\nWorkspace: {}\nActive run: {}\nProvider / model: {} / {}\nPending approvals: {}\nTools: {} · MCP servers: {}",
    node
      .map(|value| value.daemon_version.as_str())
      .unwrap_or("unknown"),
    node.map(|value| value.id.as_str()).unwrap_or("unknown"),
    session.map(|value| value.title.as_str()).unwrap_or("none"),
    session.map(|value| value.id.as_str()).unwrap_or("none"),
    session
      .map(|value| value.workspace.as_str())
      .unwrap_or("none"),
    run
      .map(|value| format!("{} · {}", value.id, value.status))
      .unwrap_or_else(|| "none".into()),
    status.provider,
    status.model,
    status.pending_approvals,
    status.tools,
    status.mcp_servers
  )
}

pub fn format_providers(daemon: &[v1::Provider], candidates: &[ClientProviderCandidate]) -> String {
  let mut lines = vec!["Daemon provider inventory".to_owned()];
  if daemon.is_empty() {
    lines.push("No providers returned by agentd".into());
  }
  for provider in daemon {
    let models = provider
      .models
      .iter()
      .map(|model| model.id.as_str())
      .collect::<Vec<_>>()
      .join(", ");
    lines.push(format!(
      "{}{} · {} · credential {} ({}) · models: {}",
      if provider.selected { "› " } else { "  " },
      provider.name,
      if provider.runtime_supported {
        "supported"
      } else {
        "not supported yet"
      },
      provider.credential_state,
      if provider.credential_environment.is_empty() {
        if provider.credential_source.is_empty() {
          "no credential"
        } else {
          provider.credential_source.as_str()
        }
      } else {
        provider.credential_environment.as_str()
      },
      if models.is_empty() { "none" } else { &models }
    ));
  }
  lines.push(String::new());
  lines.push("Client process environment candidates (not uploaded)".into());
  if candidates.is_empty() {
    lines.push("No allowlisted provider credentials detected".into());
  }
  for candidate in candidates {
    lines.push(format!("{} · {}", candidate.environment, candidate.state));
  }
  lines.join("\n")
}

pub fn format_provider_models(providers: &[v1::Provider]) -> String {
  let mut lines = vec!["Configured models".to_owned()];
  for provider in providers {
    for model in &provider.models {
      lines.push(format!(
        "{}{} · {}",
        if model.selected { "› " } else { "  " },
        model.id,
        provider.name
      ));
    }
  }
  if lines.len() == 1 {
    lines.push("No configured models".into());
  }
  lines.join("\n")
}

pub fn format_git(status: &v1::GitStatus) -> String {
  if !status.repository {
    return "This workspace is not a Git repository".into();
  }
  let mut lines = vec![format!(
    "Branch: {} · HEAD {} · upstream {} · ahead {} / behind {}",
    empty_as(status.branch.as_str(), "(detached)"),
    empty_as(status.head.as_str(), "unknown"),
    empty_as(status.upstream.as_str(), "none"),
    status.ahead,
    status.behind
  )];
  if status.entries.is_empty() {
    lines.push("Working tree clean".into());
  } else {
    lines.extend(status.entries.iter().map(|entry| {
      format!(
        "{}{} {}{}",
        if entry.index_status == "." {
          " "
        } else {
          &entry.index_status
        },
        if entry.worktree_status == "." {
          " "
        } else {
          &entry.worktree_status
        },
        entry.path,
        if entry.original_path.is_empty() {
          String::new()
        } else {
          format!(" ← {}", entry.original_path)
        }
      )
    }));
  }
  lines.join("\n")
}

fn empty_as<'a>(value: &'a str, fallback: &'a str) -> &'a str {
  if value.is_empty() { fallback } else { value }
}

pub fn format_skills(skills: &[v1::Skill]) -> String {
  if skills.is_empty() {
    return "No installed Skills".into();
  }
  skills
    .iter()
    .map(|skill| {
      format!(
        "{}{} · {} · {} files · {} bytes · sha256 {}\n  {}",
        if skill.enabled { "● " } else { "○ " },
        skill.name,
        skill.scope,
        skill.files,
        skill.total_bytes,
        skill.content_sha256,
        skill.description
      )
    })
    .collect::<Vec<_>>()
    .join("\n")
}

pub fn format_skill_inspection(inspection: &v1::SkillInspection) -> String {
  let mut lines = Vec::new();
  if let Some(skill) = &inspection.skill {
    lines.push(format_skills(std::slice::from_ref(skill)));
  }
  lines.extend(inspection.files.iter().map(|file| {
    format!(
      "{} · {} bytes · sha256 {}",
      file.path, file.bytes, file.sha256
    )
  }));
  if lines.is_empty() {
    "Skill inspection returned no metadata".into()
  } else {
    lines.join("\n")
  }
}

#[cfg(test)]
mod tests {
  use super::{ClientProviderCandidate, format_providers};

  #[test]
  fn provider_panel_separates_client_candidates_and_never_contains_secret_values() {
    let daemon = [agent_protocol::v1::Provider {
      id: "openai".into(),
      name: "OpenAI".into(),
      protocols: vec!["openai_responses".into()],
      runtime_supported: true,
      selected: true,
      credential_state: "detected".into(),
      credential_source: "daemon_environment".into(),
      credential_environment: "OPENAI_API_KEY".into(),
      models: vec![],
    }];
    let client = [ClientProviderCandidate {
      environment: "OPENAI_API_KEY".into(),
      state: "detected".into(),
    }];
    let rendered = format_providers(&daemon, &client);
    assert!(rendered.contains("Client process environment candidates (not uploaded)"));
    assert!(rendered.contains("OPENAI_API_KEY · detected"));
    assert!(!rendered.contains("test-secret"));
  }
}
