use crate::local_parse::{P0Command, parse_p0};
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
  if prompt.trim() == "/search" || prompt.trim_start().starts_with("/search ") {
    let query = prompt
      .trim_start()
      .strip_prefix("/search")
      .unwrap_or_default()
      .trim();
    crate::reading_ui::search(app, query);
    app.input.clear();
  } else if let Some(parsed) = parse_p0(&prompt) {
    match parsed {
      Ok(P0Command::Activity) => {
        app.router.navigate(ContextPanel::Activity);
        app.input.clear();
      }
      Ok(P0Command::New) => {
        send(actions, Action::NewSession)?;
        app.input.clear();
        app.save_current_draft();
      }
      Ok(P0Command::Sessions) => {
        // The F2 panel stays for browsing; this picker is the quick jump.
        let current = app.selected_session().map(|session| session.id.clone());
        let picker = crate::picker::Picker::sessions(&app.sessions, current.as_deref());
        app.open_picker(picker);
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
          let session = app.selected_session().map(|session| session.id.clone());
          match command {
            // Provider and skill inventories are pickers, not text pages.
            crate::resource_command::ResourceCommand::Providers { .. } => {
              send(
                actions,
                Action::ProviderCatalog {
                  generation: app.generation,
                },
              )?;
              app.open_picker(crate::picker::Picker::loading(
                "选择提供方 · 加载中… · Esc 取消",
              ));
              app.input.clear();
              app.save_current_draft();
            }
            crate::resource_command::ResourceCommand::SkillsList { scope } => {
              send(
                actions,
                Action::SkillsCatalog {
                  session,
                  generation: app.generation,
                  scope,
                },
              )?;
              app.open_picker(crate::picker::Picker::loading(
                "选择技能 · 加载中… · Esc 取消",
              ));
              app.input.clear();
              app.save_current_draft();
            }
            // A model choice is a picker, not a text page.
            crate::resource_command::ResourceCommand::Models => {
              send(
                actions,
                Action::ModelCatalog {
                  session,
                  generation: app.generation,
                },
              )?;
              app.open_picker(crate::picker::Picker::models());
              app.input.clear();
              app.save_current_draft();
            }
            // Diffs open a full screen reading surface instead of a side column.
            crate::resource_command::ResourceCommand::Diff { path } => {
              send(
                actions,
                Action::WorkspaceDiff {
                  session: session.unwrap_or_default(),
                  generation: app.generation,
                  path,
                },
              )?;
              app.notice = "正在读取工作区改动…".into();
              app.input.clear();
              app.save_current_draft();
            }
            command => {
              let navigation_revision = app.router.begin_request();
              send(
                actions,
                Action::Resource {
                  session,
                  generation: app.generation,
                  navigation_revision,
                  source: prompt,
                  command,
                },
              )?;
              // Clearing at submission keeps late output from erasing newer typing.
              app.input.clear();
              app.save_current_draft();
            }
          }
        }
      }
      Err(error) => app.notice = error,
    }
  } else if prompt.trim_start() == "/help" || prompt.trim_start().starts_with("/help ") {
    let command = prompt.split_whitespace().nth(1);
    app.task_detail = crate::slash_command::help_text(command);
    app.router.navigate(ContextPanel::Help);
    app.input.clear();
  } else if prompt.trim_start() == "/draft" || prompt.trim_start().starts_with("/draft ") {
    crate::draft_command::run(app, &prompt);
  } else if prompt.trim_start() == "/run" || prompt.trim_start().starts_with("/run ") {
    app.router.navigate(ContextPanel::Tasks);
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
    app.router.navigate(ContextPanel::Tasks);
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
    app.router.navigate(ContextPanel::Computer);
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
  } else if app.connection == Connection::Connected
    && (app.active_run.is_some()
      || app.input_attempt.as_ref().is_some_and(|attempt| {
        app
          .selected_session()
          .is_some_and(|session| session.id == attempt.session)
          && attempt.prompt == prompt
      }))
  {
    crate::run_input_ui::submit(app, actions, agent_core::events::QueueKind::FollowUp)?;
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
