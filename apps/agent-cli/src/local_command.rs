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

pub(crate) fn submit(app: &mut App, actions: &mpsc::Sender<Action>) -> anyhow::Result<()> {
  let prompt = app.input.text();
  if prompt.trim_start() == "/draft" || prompt.trim_start().starts_with("/draft ") {
    run_draft_command(app, &prompt);
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
