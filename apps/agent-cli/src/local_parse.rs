#[derive(Debug)]
pub(crate) enum P0Command {
  Activity,
  New,
  Sessions,
  Cancel,
  Resource(crate::resource_command::ResourceCommand),
}

pub(crate) fn parse_p0(prompt: &str) -> Option<Result<P0Command, String>> {
  let words = match shell_words::split(prompt) {
    Ok(words) => words,
    Err(error) if prompt.trim_start().starts_with('/') => {
      return Some(Err(format!("Invalid command quoting: {error}")));
    }
    Err(_) => return None,
  };
  let root = words.first()?.as_str();
  let args = &words[1..];
  let parsed = match root {
    "/plan" | "/changes" | "/undo" => crate::plan_command::parse(root, args)
      .map(crate::resource_command::ResourceCommand::Workflow)
      .map(P0Command::Resource),
    "/memory" => crate::memory_command::parse(args)
      .map(crate::resource_command::ResourceCommand::Memory)
      .map(P0Command::Resource),
    "/new" if args.is_empty() => Ok(P0Command::New),
    "/sessions" if args.is_empty() => Ok(P0Command::Sessions),
    "/cancel" if args.is_empty() => Ok(P0Command::Cancel),
    "/activity" if args.is_empty() => {
      return Some(Ok(P0Command::Activity));
    }
    "/status" if args.is_empty() => Ok(P0Command::Resource(
      crate::resource_command::ResourceCommand::Status,
    )),
    "/context" | "/compact" | "/mode" => {
      crate::context_command::parse(root, args).map(P0Command::Resource)
    }
    "/permissions" if args.is_empty() => Ok(P0Command::Resource(
      crate::resource_command::ResourceCommand::Permissions,
    )),
    "/mcp" if args.is_empty() => Ok(P0Command::Resource(
      crate::resource_command::ResourceCommand::Mcp,
    )),
    "/tools" if args.is_empty() => Ok(P0Command::Resource(
      crate::resource_command::ResourceCommand::Tools,
    )),
    "/providers" => parse_providers(args),
    "/models" | "/model" => crate::model_command::parse(args).map(P0Command::Resource),
    "/git" if args.is_empty() => Ok(P0Command::Resource(
      crate::resource_command::ResourceCommand::Git,
    )),
    "/diff" if args.len() <= 1 => Ok(P0Command::Resource(
      crate::resource_command::ResourceCommand::Diff {
        path: args.first().cloned().unwrap_or_default(),
      },
    )),
    "/skills" => parse_skills(args),
    _ => return None,
  };
  Some(parsed)
}

fn parse_providers(args: &[String]) -> Result<P0Command, String> {
  use crate::resource_command::ResourceCommand;
  match args {
    [] => Ok(P0Command::Resource(ResourceCommand::Providers {
      client_candidates: crate::resource_command::candidate_environment_names(),
    })),
    [verb, provider, environment] if verb == "upload" => {
      Ok(P0Command::Resource(ResourceCommand::ProviderUpload {
        provider: provider.clone(),
        environment: environment.clone(),
      }))
    }
    [verb, provider] if verb == "remove" => {
      Ok(P0Command::Resource(ResourceCommand::ProviderRemove {
        provider: provider.clone(),
      }))
    }
    [verb, ..] if verb == "upload" || verb == "remove" => {
      Err("Usage: /providers upload PROVIDER ENV_NAME | /providers remove PROVIDER".into())
    }
    _ => Err("Usage: /providers [upload PROVIDER ENV_NAME | remove PROVIDER]".into()),
  }
}

fn parse_skills(args: &[String]) -> Result<P0Command, String> {
  use crate::resource_command::ResourceCommand;
  let valid_scope = |scope: &str| matches!(scope, "daemon_user" | "workspace");
  match args {
    [] => {
      Ok(P0Command::Resource(ResourceCommand::SkillsList {
        scope: "daemon_user".into(),
      }))
    }
    [verb] if verb == "list" => {
      Ok(P0Command::Resource(ResourceCommand::SkillsList {
        scope: "daemon_user".into(),
      }))
    }
    [verb, scope] if verb == "list" && valid_scope(scope) => {
      Ok(P0Command::Resource(ResourceCommand::SkillsList {
        scope: scope.clone(),
      }))
    }
    [verb, source] if verb == "inspect" => Ok(P0Command::Resource(ResourceCommand::SkillInspect {
      source: source.clone(),
      scope: "daemon_user".into(),
    })),
    [verb, source, scope] if verb == "inspect" && valid_scope(scope) => {
      Ok(P0Command::Resource(ResourceCommand::SkillInspect {
        source: source.clone(),
        scope: scope.clone(),
      }))
    }
    [verb, source, scope, expected] if verb == "install" && valid_scope(scope) => {
      Ok(P0Command::Resource(ResourceCommand::SkillInstall {
        source: source.clone(),
        scope: scope.clone(),
        expected_sha256: expected.clone(),
      }))
    }
    [verb, name, scope, expected] if verb == "remove" && valid_scope(scope) => {
      Ok(P0Command::Resource(ResourceCommand::SkillRemove {
        name: name.clone(),
        scope: scope.clone(),
        expected_sha256: expected.clone(),
      }))
    }
    _ => Err("Usage: /skills list [daemon_user|workspace] · inspect SOURCE [SCOPE] · install SOURCE SCOPE EXPECTED_SHA256 · remove NAME SCOPE EXPECTED_SHA256".into()),
  }
}
