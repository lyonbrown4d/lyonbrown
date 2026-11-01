use clap::Parser;
use tokio::sync::mpsc;

use crate::{
  backend::Action,
  input::send,
  model::{App, Connection, ContextPanel},
};

#[derive(Parser)]
#[command(name = "task", disable_help_flag = false)]
struct TaskLine {
  #[command(subcommand)]
  command: crate::task_command::TaskCommand,
}

#[derive(Parser)]
#[command(name = "run")]
struct RunLine {
  #[command(subcommand)]
  command: crate::run_command::RunCommand,
}

pub(crate) fn submit(app: &mut App, actions: &mpsc::Sender<Action>) -> anyhow::Result<()> {
  let prompt = app.input.text();
  if let Some(parsed) = parse_p0(&prompt) {
    match parsed {
      Ok(P0Command::New) => {
        send(actions, Action::NewSession)?;
        app.input.clear();
        app.save_current_draft();
      }
      Ok(P0Command::Sessions) => {
        app.sessions_cursor = app.selected;
        app.panel = ContextPanel::Sessions;
        app.input.clear();
        app.save_current_draft();
      }
      Ok(P0Command::Cancel) => {
        if let Some(run) = app.active_run.clone() {
          send(actions, Action::Cancel(run))?;
        } else {
          app.notice = "当前没有运行中的任务。".into();
        }
        app.input.clear();
        app.save_current_draft();
      }
      Ok(P0Command::Resource(command)) => {
        if matches!(
          command,
          crate::resource_command::ResourceCommand::Git
            | crate::resource_command::ResourceCommand::Diff { .. }
            | crate::resource_command::ResourceCommand::SkillsList { .. }
            | crate::resource_command::ResourceCommand::SkillInspect { .. }
            | crate::resource_command::ResourceCommand::SkillInstall { .. }
            | crate::resource_command::ResourceCommand::SkillRemove { .. }
            | crate::resource_command::ResourceCommand::Status
        ) && app.selected_session().is_none()
        {
          app.notice = "请先创建或选择一个会话。".into();
        } else {
          send(
            actions,
            Action::Resource {
              session: app.selected_session().map(|session| session.id.clone()),
              generation: app.generation,
              source: prompt,
              command,
            },
          )?;
        }
      }
      Err(error) => app.notice = error,
    }
  } else if prompt.trim_start() == "/help" || prompt.trim_start().starts_with("/help ") {
    let command = prompt.split_whitespace().nth(1);
    app.task_detail = crate::slash_command::help_text(command);
    app.panel = ContextPanel::Help;
    app.input.clear();
  } else if prompt.trim_start() == "/draft" || prompt.trim_start().starts_with("/draft ") {
    run_draft_command(app, &prompt);
  } else if prompt.trim_start() == "/run" || prompt.trim_start().starts_with("/run ") {
    app.panel = ContextPanel::Tasks;
    match shell_words::split(&prompt) {
      Ok(mut words) => {
        if let Some(first) = words.first_mut() {
          *first = "run".into();
        }
        match RunLine::try_parse_from(words) {
          Ok(parsed) => send(
            actions,
            Action::Run {
              session: app.selected_session().map(|session| session.id.clone()),
              generation: app.generation,
              source: prompt,
              command: parsed.command,
            },
          )?,
          Err(error) => app.task_detail = error.to_string(),
        }
      }
      Err(error) => app.task_detail = format!("Invalid run command quoting: {error}"),
    }
  } else if prompt.trim_start() == "/task" || prompt.trim_start().starts_with("/task ") {
    app.panel = ContextPanel::Tasks;
    match shell_words::split(&prompt) {
      Ok(mut words) => {
        if let Some(command) = words.first_mut() {
          *command = "task".into();
        }
        match TaskLine::try_parse_from(words) {
          Ok(parsed) => {
            let session = app.selected_session().map(|session| session.id.clone());
            let append = matches!(
              &parsed.command,
              crate::task_command::TaskCommand::List { after: Some(_), .. }
            );
            send(
              actions,
              Action::Task {
                session,
                generation: app.generation,
                source: Some(prompt.clone()),
                append,
                command: parsed.command,
              },
            )?;
          }
          Err(error) => app.task_detail = error.to_string(),
        }
      }
      Err(error) => app.task_detail = format!("Invalid task command quoting: {error}"),
    }
  } else if let Some(command) = crate::computer::parse_local(&prompt) {
    app.panel = ContextPanel::Computer;
    match command {
      Ok(command) => {
        send(
          actions,
          Action::Computer {
            session: app.selected_session().map(|session| session.id.clone()),
            command,
          },
        )?;
        app.input.clear();
      }
      Err(error) => app.computer_detail = error.to_string(),
    }
  } else if prompt.trim_start().starts_with('/') {
    let command = prompt
      .split_whitespace()
      .next()
      .unwrap_or("/")
      .trim_start_matches('/');
    app.notice = if crate::slash_command::find(command).is_some() {
      format!("/{command} 尚未可用。输入 /help 查看当前支持的命令。")
    } else {
      format!("未知命令 /{command}。输入 / 查看命令，或 /help 查看帮助。")
    };
  } else if app.connection != Connection::Connected {
    app.notice = "Disconnected: Ctrl+R reconnects. Your draft is preserved.".into();
  } else if let Some(session) = app.selected_session() {
    if !app.submitting && !prompt.trim().is_empty() {
      send(
        actions,
        Action::Submit {
          session: session.id.clone(),
          prompt,
          generation: app.generation,
        },
      )?;
      app.submitting = true;
    }
  } else {
    app.notice = "Ctrl+N creates a session before sending.".into();
  }
  Ok(())
}

#[derive(Debug)]
enum P0Command {
  New,
  Sessions,
  Cancel,
  Resource(crate::resource_command::ResourceCommand),
}

fn parse_p0(prompt: &str) -> Option<Result<P0Command, String>> {
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
    "/new" if args.is_empty() => Ok(P0Command::New),
    "/sessions" if args.is_empty() => Ok(P0Command::Sessions),
    "/cancel" if args.is_empty() => Ok(P0Command::Cancel),
    "/status" if args.is_empty() => Ok(P0Command::Resource(
      crate::resource_command::ResourceCommand::Status,
    )),
    "/providers" => parse_providers(args),
    "/models" | "/model" if args.is_empty() => Ok(P0Command::Resource(
      crate::resource_command::ResourceCommand::Models,
    )),
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

fn run_draft_command(app: &mut App, prompt: &str) {
  let words = match shell_words::split(prompt) {
    Ok(words) => words,
    Err(error) => {
      app.draft_detail = format!("Invalid draft command quoting: {error}");
      app.panel = ContextPanel::Drafts;
      return;
    }
  };
  match words.as_slice() {
    [_, command] if command == "list" => {
      app.draft_detail = if app.drafts.is_empty() && app.unbound_draft.is_empty() {
        "没有保存的草稿。".into()
      } else {
        app
          .drafts
          .iter()
          .map(|(session, draft)| {
            let known = app.sessions.iter().any(|value| value.id == *session);
            format!(
              "{} {session} · {} 字符 · /draft recover {session}",
              if known {
                "会话草稿"
              } else {
                "原会话当前不可用"
              },
              draft.chars().count()
            )
          })
          .chain((!app.unbound_draft.is_empty()).then(|| {
            format!(
              "未绑定草稿 · {} 字符 · /draft recover unbound",
              app.unbound_draft.chars().count()
            )
          }))
          .collect::<Vec<_>>()
          .join("\n")
      };
      app.input.clear();
      app.recovered_draft_from = None;
    }
    [_, command, session] if command == "recover" => {
      let draft = if session == "unbound" {
        (!app.unbound_draft.is_empty()).then(|| app.unbound_draft.clone())
      } else {
        app.drafts.get(session).cloned()
      };
      if let Some(draft) = draft {
        app.input = crate::composer::Composer::from(draft.as_str());
        app.recovered_draft_from = Some(session.clone());
        app.draft_detail =
          format!("已从会话 {session} 恢复草稿到编辑器。检查内容和当前会话后按 Enter 才会提交。");
      } else {
        app.draft_detail = format!("找不到会话 {session} 的保存草稿。没有发送任何内容。");
      }
    }
    _ => {
      app.draft_detail = "用法：/draft list · /draft recover SESSION_ID".into();
    }
  }
  app.panel = ContextPanel::Drafts;
}

pub(crate) fn request_task_page(
  app: &App,
  actions: &mpsc::Sender<Action>,
  after: &str,
) -> anyhow::Result<()> {
  let Some(session) = app.selected_session().map(|session| session.id.clone()) else {
    return Ok(());
  };
  send(
    actions,
    Action::Task {
      session: Some(session.clone()),
      generation: app.generation,
      source: None,
      append: !after.is_empty(),
      command: crate::task_command::TaskCommand::List {
        session,
        after: (!after.is_empty()).then(|| after.to_owned()),
        page_size: 50,
      },
    },
  )
}

pub(crate) fn load_tasks(app: &App, actions: &mpsc::Sender<Action>) -> anyhow::Result<()> {
  request_task_page(app, actions, "")
}
