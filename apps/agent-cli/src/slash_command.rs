//! Registry for client-side commands understood by the interactive composer.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
  pub name: &'static str,
  pub usage: &'static str,
  pub summary: &'static str,
}

pub const COMMANDS: &[CommandSpec] = &[
  CommandSpec {
    name: "cancel",
    usage: "/cancel",
    summary: "Cancel the active run",
  },
  CommandSpec {
    name: "context",
    usage: "/context",
    summary: "Show current context status",
  },
  CommandSpec {
    name: "computer",
    usage: "/computer <targets|status|attach|detach>",
    summary: "Manage an explicitly selected local computer target",
  },
  CommandSpec {
    name: "draft",
    usage: "/draft <list|recover SESSION_ID>",
    summary: "List or recover locally saved drafts",
  },
  CommandSpec {
    name: "diff",
    usage: "/diff",
    summary: "Inspect workspace changes",
  },
  CommandSpec {
    name: "git",
    usage: "/git",
    summary: "Open the Git workbench",
  },
  CommandSpec {
    name: "help",
    usage: "/help [command]",
    summary: "Show available commands and usage",
  },
  CommandSpec {
    name: "models",
    usage: "/models [list|select MODEL EXECUTION]",
    summary: "Browse and select configured models",
  },
  CommandSpec {
    name: "mcp",
    usage: "/mcp",
    summary: "Inspect MCP servers",
  },
  CommandSpec {
    name: "model",
    usage: "/model [list|select MODEL EXECUTION]",
    summary: "Inspect and select the model",
  },
  CommandSpec {
    name: "new",
    usage: "/new",
    summary: "Create a session",
  },
  CommandSpec {
    name: "permissions",
    usage: "/permissions",
    summary: "Inspect permission policy",
  },
  CommandSpec {
    name: "providers",
    usage: "/providers",
    summary: "Inspect and configure model providers",
  },
  CommandSpec {
    name: "run",
    usage: "/run <inspect|abandon|continue> ...",
    summary: "Inspect or explicitly resolve an uncertain run",
  },
  CommandSpec {
    name: "sessions",
    usage: "/sessions",
    summary: "Browse and switch sessions",
  },
  CommandSpec {
    name: "skills",
    usage: "/skills",
    summary: "Browse installed and available Skills",
  },
  CommandSpec {
    name: "status",
    usage: "/status",
    summary: "Show connection and run status",
  },
  CommandSpec {
    name: "tools",
    usage: "/tools",
    summary: "Inspect available tools",
  },
  CommandSpec {
    name: "task",
    usage: "/task <list|get|dispatch|cancel|reconcile|accept> ...",
    summary: "Manage persistent distributed tasks",
  },
];

pub fn find(name: &str) -> Option<&'static CommandSpec> {
  COMMANDS.iter().find(|command| command.name == name)
}

pub fn filter(query: &str) -> Vec<&'static CommandSpec> {
  let query = query.trim_start_matches('/').to_ascii_lowercase();
  if query.is_empty() {
    let mut commands = COMMANDS.iter().collect::<Vec<_>>();
    commands.sort_by_key(|command| command.name);
    return commands;
  }
  let mut matches = COMMANDS
    .iter()
    .filter(|command| {
      command.name.starts_with(&query) || subsequence(&command.name.to_ascii_lowercase(), &query)
    })
    .collect::<Vec<_>>();
  matches.sort_by_key(|command| (!command.name.starts_with(&query), command.name));
  matches
}

fn subsequence(candidate: &str, query: &str) -> bool {
  let mut chars = candidate.chars();
  query.chars().all(|needle| chars.any(|ch| ch == needle))
}

pub fn help_text(command: Option<&str>) -> String {
  let command = command.map(|value| value.trim_start_matches('/'));
  match command.and_then(find) {
    Some(spec) => format!(
      "{}\n{}\n{}",
      spec.usage, spec.summary, "命令由 CLI 本地解析；格式错误不会作为提示发送给模型。"
    ),
    None if command.is_some() => format!(
      "未知命令 /{}。输入 /help 查看可用命令。",
      command.unwrap_or_default()
    ),
    None => COMMANDS
      .iter()
      .map(|spec| {
        format!(
          "{:<12} {}\n             {}",
          format!("/{}", spec.name),
          spec.usage,
          spec.summary
        )
      })
      .collect::<Vec<_>>()
      .join("\n\n"),
  }
}

#[cfg(test)]
mod tests {
  use super::{filter, find};

  #[test]
  fn command_filter_supports_prefix_and_fuzzy_matching() {
    assert_eq!(filter("/tas")[0].name, "task");
    assert!(filter("/cmpt").iter().any(|spec| spec.name == "computer"));
    assert!(filter("/not-a-command").is_empty());
  }

  #[test]
  fn registry_only_resolves_supported_commands() {
    assert!(find("help").is_some());
    assert!(find("provider").is_none());
  }
}
