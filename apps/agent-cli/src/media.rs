//! Explicit terminal image selection. Bytes stay in transient per-session drafts.
mod backend;
use crate::{
  backend::Action,
  model::{App, Connection},
};
use agent_client::media::ImageSubmission;
use std::path::PathBuf;
use tokio::sync::mpsc;
pub enum Request {
  Attach {
    session: String,
    path: PathBuf,
  },
  Submit {
    session: String,
    generation: u64,
    submission: ImageSubmission,
  },
}
pub enum Reply {
  Attached {
    session: String,
    name: String,
    image: agent_media::ImageAttachment,
  },
  Rejected {
    session: String,
    request_id: String,
  },
  Confirmed {
    session: String,
    request_id: String,
  },
}
pub(crate) use backend::execute;

pub(crate) fn has_images(app: &App) -> bool {
  app
    .selected_session()
    .and_then(|session| app.image_drafts.get(&session.id))
    .is_some_and(|draft| !draft.attachments.is_empty())
}
pub(crate) fn command(
  app: &mut App,
  actions: &mpsc::Sender<Action>,
  prompt: &str,
) -> anyhow::Result<bool> {
  let Some(command) = prompt
    .split_whitespace()
    .next()
    .filter(|token| matches!(*token, "/attach" | "/detach"))
  else {
    return Ok(false);
  };
  let Some(session) = app.selected_session().map(|session| session.id.clone()) else {
    app.notice = "请先创建或选择会话，再添加图片。".into();
    return Ok(true);
  };
  let arguments = match shell_words::split(prompt) {
    Ok(arguments) => arguments,
    Err(_) => {
      app.notice = "图片路径引号不完整；使用 /attach \"本地图片路径\"。".into();
      return Ok(true);
    }
  };
  if command == "/attach" && arguments.len() == 2 {
    if app
      .image_drafts
      .get(&session)
      .is_some_and(|draft| draft.pending.is_some())
    {
      app.notice = "图片请求尚未确认；先使用原请求重试或检查任务记录。".into();
      return Ok(true);
    }
    crate::input::send(
      actions,
      Action::Media(Request::Attach {
        session,
        path: PathBuf::from(&arguments[1]),
      }),
    )?;
  } else if command == "/detach" && arguments.len() == 2 {
    let result = arguments[1]
      .parse::<usize>()
      .ok()
      .filter(|index| *index > 0)
      .ok_or_else(|| anyhow::anyhow!("使用 /detach 图片序号，从 1 开始"))
      .and_then(|index| {
        app
          .image_drafts
          .entry(session)
          .or_default()
          .remove(index - 1)
      });
    app.notice = match result {
      Ok(()) => "已移除图片。".into(),
      Err(error) => error.to_string(),
    };
  } else {
    app.notice =
      "使用 /attach \"本地图片路径\" 或 /detach 序号；最多 8 图、单图 16 MiB、合计 32 MiB。".into();
  }
  app.input.clear();
  Ok(true)
}
pub(crate) fn submit(
  app: &mut App,
  actions: &mpsc::Sender<Action>,
  prompt: &str,
) -> anyhow::Result<()> {
  if app.submitting {
    return Ok(());
  }
  if app.connection != Connection::Connected || app.active_run.is_some() {
    app.notice = "图片仅可作为新任务输入；请连接并等待当前任务结束。附件已保留。".into();
    return Ok(());
  }
  let Some(session) = app.selected_session().map(|session| session.id.clone()) else {
    return Ok(());
  };
  let draft = app.image_drafts.entry(session.clone()).or_default();
  match draft.prepare(Some(&session), prompt, uuid::Uuid::now_v7().to_string()) {
    Ok(submission) => {
      crate::input::send(
        actions,
        Action::Media(Request::Submit {
          session,
          generation: app.generation,
          submission,
        }),
      )?;
      app.submitting = true;
      app.notice = "正在提交图片；收到 agentd 确认前保留附件和请求标识。".into();
    }
    Err(error) => app.notice = error.to_string(),
  }
  Ok(())
}
pub(crate) fn apply(app: &mut App, reply: Reply) {
  match reply {
    Reply::Attached {
      session,
      name,
      image,
    } => {
      let total = app
        .image_drafts
        .values()
        .flat_map(|draft| &draft.attachments)
        .map(|entry| entry.image.data.len())
        .sum::<usize>();
      if total.saturating_add(image.data.len()) > agent_media::MAX_TOTAL_BYTES {
        app.notice = "本地图片草稿总量已达 32 MiB；请先移除附件。".into();
        return;
      }
      let draft = app.image_drafts.entry(session).or_default();
      app.notice = match draft.add(name, image) {
        Ok(()) => format!(
          "已添加 {} 张图片。/detach 序号可移除；输入任务后发送。",
          draft.attachments.len()
        ),
        Err(error) => error.to_string(),
      };
    }
    Reply::Rejected {
      session,
      request_id,
    } => {
      if let Some(draft) = app.image_drafts.get_mut(&session) {
        draft.reject(&request_id);
      }
    }
    Reply::Confirmed {
      session,
      request_id,
    } => {
      if let Some(draft) = app.image_drafts.get_mut(&session) {
        draft.acknowledge(&request_id);
      }
    }
  }
}
