use crate::{backend::Action, input::send, model::App};
use tokio::sync::mpsc;
/// Confirming a picker row only dispatches a daemon action; it never applies state locally.
pub(crate) fn confirm(app: &mut App, actions: &mpsc::Sender<Action>) -> anyhow::Result<()> {
  let Some(value) = app
    .picker
    .as_ref()
    .and_then(|picker| picker.selected_value().cloned())
  else {
    app.notice = "没有可用的选项。".into();
    app.close_picker();
    return Ok(());
  };
  app.close_picker();
  match value {
    crate::picker::PickerValue::History { session, prompt } => {
      if app
        .selected_session()
        .is_some_and(|selected| selected.id == session)
      {
        app.interaction.reset_navigation(&session);
        app.input.replace_text(&prompt);
        app.notice = "已回填历史输入；检查后按 Enter 提交。".into();
      }
    }
    crate::picker::PickerValue::Model { model, execution } => {
      let Some(session) = app.selected_session().map(|session| session.id.clone()) else {
        app.notice = "请先创建或选择一个会话。".into();
        return Ok(());
      };
      let navigation_revision = app.router.begin_request();
      send(
        actions,
        Action::Resource {
          session: Some(session),
          generation: app.generation,
          navigation_revision,
          source: String::new(),
          command: crate::resource_command::ResourceCommand::ModelSelect { model, execution },
        },
      )?;
      app.notice = "正在切换模型…".into();
    }
    crate::picker::PickerValue::Session { id } => {
      let Some(index) = app.sessions.iter().position(|session| session.id == id) else {
        app.notice = "该会话已不存在。".into();
        return Ok(());
      };
      app.select_session(index);
      send(
        actions,
        Action::Select {
          session: id,
          generation: app.generation,
        },
      )?;
    }
    crate::picker::PickerValue::Provider {
      provider,
      environment,
    } => {
      if environment.is_empty() {
        app.notice = format!("{provider} 没有检测到可上传的凭据环境变量。");
        return Ok(());
      }
      let navigation_revision = app.router.begin_request();
      send(
        actions,
        Action::Resource {
          session: None,
          generation: app.generation,
          navigation_revision,
          source: String::new(),
          command: crate::resource_command::ResourceCommand::ProviderUpload {
            provider,
            environment,
          },
        },
      )?;
      app.notice = "正在上传凭据…".into();
    }
    crate::picker::PickerValue::Skill { name, scope } => {
      let session = app.selected_session().map(|session| session.id.clone());
      let navigation_revision = app.router.begin_request();
      send(
        actions,
        Action::Resource {
          session,
          generation: app.generation,
          navigation_revision,
          source: String::new(),
          command: crate::resource_command::ResourceCommand::SkillInspect {
            source: name,
            scope,
          },
        },
      )?;
      app.notice = "正在读取技能…".into();
    }
  }
  Ok(())
}
