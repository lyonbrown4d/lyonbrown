//! Client presentation of daemon-confirmed queued input.
use crate::{
  backend::Action,
  input::send,
  model::{App, ContextPanel},
};
use agent_core::events::QueueKind;
use agent_protocol::v1;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputAttempt {
  pub session: String,
  pub run: String,
  pub prompt: String,
  pub kind: QueueKind,
  pub request_id: String,
}
pub(crate) fn submit(
  app: &mut App,
  actions: &mpsc::Sender<Action>,
  kind: QueueKind,
) -> anyhow::Result<()> {
  if app.submitting {
    return Ok(());
  }
  let prompt = app.input.text();
  if prompt.trim().is_empty() {
    return Ok(());
  }
  if prompt.trim_start().starts_with('/') {
    app.notice = "斜杠命令请按 Enter 在本地处理；没有发送纠偏输入。".into();
    return Ok(());
  }
  if app.connection != crate::model::Connection::Connected {
    app.notice = "连接已断开，草稿与原请求身份保留。".into();
    return Ok(());
  }
  let Some(session) = app.selected_session().map(|session| session.id.clone()) else {
    return Ok(());
  };
  let attempt = if let Some(attempt) = app
    .input_attempt
    .as_ref()
    .filter(|attempt| attempt.session == session && attempt.prompt == prompt)
  {
    attempt.clone()
  } else {
    let Some(run) = app.active_run.clone() else {
      app.notice = "当前没有可接收纠偏输入的 Run。".into();
      return Ok(());
    };
    InputAttempt {
      session,
      run,
      prompt,
      kind,
      request_id: uuid::Uuid::now_v7().to_string(),
    }
  };
  send(
    actions,
    Action::SubmitInput {
      attempt: attempt.clone(),
      generation: app.generation,
    },
  )?;
  app.input_attempt = Some(attempt);
  app.submitting = true;
  app.notice = "正在等待 agentd 确认输入；确认前保留草稿。".into();
  Ok(())
}
pub(crate) fn open(app: &mut App, actions: &mpsc::Sender<Action>) -> anyhow::Result<()> {
  app.router.toggle(ContextPanel::Inputs);
  if let Some(run) = app
    .active_run
    .clone()
    .or_else(|| app.input_receipts.last().map(|value| value.run_id.clone()))
  {
    send(
      actions,
      Action::ListInputs {
        run,
        generation: app.generation,
      },
    )?;
  }
  Ok(())
}
pub(crate) fn cancel_selected(app: &mut App, actions: &mpsc::Sender<Action>) -> anyhow::Result<()> {
  if let Some(receipt) = app.input_receipts.get(app.input_selected) {
    send(
      actions,
      Action::CancelInput {
        run: receipt.run_id.clone(),
        entry: receipt.entry_id.clone(),
        generation: app.generation,
      },
    )?;
  }
  Ok(())
}
pub(crate) fn detail(app: &App) -> ratatui::text::Text<'static> {
  let mut lines = vec![
    "Enter：排入当前回复之后 · Ctrl+S：纠偏 · Delete：撤销所选待处理输入".into(),
    "已接收不代表本轮已采纳；运行边界之后的待处理输入保留给下一次显式运行。".into(),
  ];
  for (index, receipt) in app.input_receipts.iter().enumerate() {
    let state = match v1::RunInputState::try_from(receipt.state).ok() {
      Some(v1::RunInputState::Pending) => "待处理",
      Some(v1::RunInputState::Consumed) => "已采纳",
      Some(v1::RunInputState::Cancelled) => "已撤销",
      Some(v1::RunInputState::CancelRequested) => "撤销未确认",
      _ => "状态未知",
    };
    let kind = if receipt.kind == v1::RunInputKind::Steer as i32 {
      "纠偏"
    } else {
      "排队"
    };
    let prompt = crate::view_model::safe_text(&receipt.prompt);
    let suffix = if receipt.prompt_truncated {
      "…（预览已截断）"
    } else {
      ""
    };
    lines.push(format!(
      "{} {kind} · {state} · {}\n{prompt}{suffix}",
      if index == app.input_selected {
        "›"
      } else {
        " "
      },
      receipt.request_id
    ));
  }
  if app.input_receipts.is_empty() {
    lines.push("没有已返回的输入记录。".into());
  }
  ratatui::text::Text::raw(lines.join("\n\n"))
}

pub(crate) fn apply_event(app: &mut App, metadata: &v1::Event) {
  use agent_core::events::{AgentEvent, DecodedEvent, QueueStatus, decode};
  let Ok(DecodedEvent::Known(AgentEvent::QueueUpdated {
    entry_id,
    kind,
    status,
    prompt,
    request_id,
  })) = decode(&metadata.kind, &metadata.payload_json)
  else {
    return;
  };
  let state = match status {
    QueueStatus::Pending => v1::RunInputState::Pending,
    QueueStatus::Consumed => v1::RunInputState::Consumed,
    QueueStatus::Cancelled => v1::RunInputState::Cancelled,
    QueueStatus::CancelRequested => v1::RunInputState::CancelRequested,
  } as i32;
  if let Some(receipt) = app
    .input_receipts
    .iter_mut()
    .find(|value| value.entry_id == entry_id)
  {
    if !terminal_input_state(receipt.state) || terminal_input_state(state) {
      receipt.state = state;
    }
  } else if let Some(request_id) = request_id {
    app.input_receipts.push(v1::RunInputReceipt {
      run_id: metadata.run_id.clone(),
      request_id,
      entry_id,
      kind: match kind {
        QueueKind::Steer => v1::RunInputKind::Steer,
        QueueKind::FollowUp => v1::RunInputKind::FollowUp,
      } as i32,
      state,
      prompt: prompt.unwrap_or_default(),
      prompt_truncated: false,
    });
  }
}

pub(crate) fn terminal_input_state(state: i32) -> bool {
  matches!(
    v1::RunInputState::try_from(state).ok(),
    Some(v1::RunInputState::Consumed | v1::RunInputState::Cancelled)
  )
}
