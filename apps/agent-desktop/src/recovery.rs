mod view;

#[cfg(test)]
#[path = "../tests/internal/run_recovery.rs"]
mod tests;

use agent_protocol::v1;
use gpui_kit::{component::input::TextareaState, *};

use crate::{Desktop, connection::Update};

/// Per-chat presentation and an exact request retained for explicit retries.
/// No old Harness state or authority is restored here.
///
/// Held by the per-tab `DesktopViewModel`, so it travels with its chat tab.
pub(crate) struct RecoveryState {
  pub run_id: String,
  pub visible: bool,
  pub report: Option<v1::RunRecovery>,
  pub pending: bool,
  pub inspected: bool,
  pub error: Option<String>,
  note: Entity<TextareaState>,
  prompt: Entity<TextareaState>,
  intent: Option<v1::RecoverRunRequest>,
}

impl Desktop {
  pub(super) fn open_recovery(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
    if self.navigation.connecting || self.persistence_loading || self.vm.busy {
      return;
    }
    if self
      .vm
      .recovery
      .as_ref()
      .is_none_or(|state| state.run_id != id)
    {
      self.vm.recovery = Some(RecoveryState {
        run_id: id,
        visible: true,
        report: None,
        pending: false,
        inspected: false,
        error: None,
        note: cx.new(|cx| {
          TextareaState::new(window, cx)
            .placeholder("记录已核对的文件、命令或外部服务结果")
            .submit_on_enter(false)
        }),
        prompt: cx.new(|cx| {
          TextareaState::new(window, cx)
            .placeholder("新会话的明确输入；不会复制原运行")
            .submit_on_enter(false)
        }),
        intent: None,
      });
    }
    if let Some(state) = &mut self.vm.recovery {
      state.visible = true;
    }
    self.inspect_recovery(cx);
  }

  pub(super) fn inspect_recovery(&mut self, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let Some(state) = self.vm.recovery.as_mut().filter(|state| !state.pending) else {
      return;
    };
    state.pending = true;
    state.inspected = false;
    state.error = None;
    let id = state.run_id.clone();
    let sender = self.scoped_sender();
    self.navigation.pending_requests += 1;
    self.runtime.spawn(async move {
      let result = async {
        anyhow::ensure!(
          client
            .node()
            .await?
            .capabilities
            .iter()
            .any(|value| value == "runs.recover"),
          "daemon does not support run recovery"
        );
        client.run_recovery(&id).await
      }
      .await;
      let update = match result {
        Ok(report) => Update::Recovery(id, Box::new(report)),
        Err(error) => Update::RecoveryError(id, format!("无法核对异常运行：{error}")),
      };
      let _ = sender.send(update).await;
      sender.finish_request().await;
    });
    cx.notify();
  }

  pub(super) fn resolve_recovery(&mut self, action: v1::RunRecoveryAction, cx: &mut Context<Self>) {
    if self.navigation.connecting || self.persistence_loading || self.vm.active_run.is_some() {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    let Some(state) = self
      .vm
      .recovery
      .as_mut()
      .filter(|state| !state.pending && state.inspected)
    else {
      return;
    };
    let Some(report) = &state.report else { return };
    if !report.allowed_actions.contains(&(action as i32)) {
      return;
    }
    let request = if let Some(intent) = &state.intent {
      if intent.action != action as i32 {
        return;
      }
      intent.clone()
    } else {
      let note = state.note.read(cx).value().to_string();
      let prompt = if action == v1::RunRecoveryAction::ContinueNewSession {
        state.prompt.read(cx).value().to_string()
      } else {
        String::new()
      };
      if note.trim().is_empty()
        || note.len() > 4096
        || (action == v1::RunRecoveryAction::ContinueNewSession && prompt.trim().is_empty())
        || prompt.len() > 1_048_576
      {
        state.error =
          Some("填写核对记录（最多 4096 字节）；继续还需明确的新输入（最多 1 MiB）。".into());
        cx.notify();
        return;
      }
      v1::RecoverRunRequest {
        run_id: state.run_id.clone(),
        owner_epoch: report.owner_epoch,
        action: action as i32,
        idempotency_key: uuid::Uuid::now_v7().to_string(),
        note,
        prompt,
      }
    };
    state.intent = Some(request.clone());
    state.pending = true;
    state.inspected = false;
    state.error = None;
    let id = state.run_id.clone();
    let sender = self.scoped_sender();
    self.navigation.pending_requests += 1;
    self.runtime.spawn(async move {
      // Never resubmit on a transport error. A later click must inspect first
      // and reuse every field of this retained intent.
      let update = match client.recover_run(request).await {
        Ok(result) => match client.run_recovery(&id).await {
          Ok(report) => Update::Recovery(id, Box::new(report)),
          Err(_) => Update::Recovery(
            id,
            Box::new(v1::RunRecovery {
              run: result.original_run.clone(),
              resolution: Some(result),
              fresh_session_required: true,
              ..Default::default()
            }),
          ),
        },
        Err(error) => Update::RecoveryError(
          id,
          format!("恢复结果可能未知：{error}。先刷新核对；本次请求身份已保留，不会自动重发。"),
        ),
      };
      let _ = sender.send(update).await;
      sender.finish_request().await;
    });
    cx.notify();
  }

  pub(super) fn apply_recovery(
    &mut self,
    id: String,
    report: v1::RunRecovery,
    cx: &mut Context<Self>,
  ) {
    let Some(state) = self.vm.recovery.as_mut().filter(|state| state.run_id == id) else {
      return;
    };
    if report.run.as_ref().is_none_or(|run| {
      self.vm.selected.as_deref() != Some(run.session_id.as_str()) || run.id != id
    }) {
      state.pending = false;
      state.inspected = false;
      state.error = Some("恢复报告与当前会话不一致，请重新核对。".into());
      return;
    }
    if let Some(session) = report
      .resolution
      .as_ref()
      .and_then(|result| result.session.clone())
    {
      self.sessions.retain(|previous| previous.id != session.id);
      self.sessions.insert(0, session);
    }
    state.report = Some(report);
    state.pending = false;
    state.inspected = true;
    state.error = None;
    cx.notify();
  }

  pub(super) fn open_recovery_tasks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.tasks.read(cx).task_pending {
      if let Some(state) = &mut self.vm.recovery {
        state.error = Some("等待当前 Task 请求结束后再打开核对列表。".into());
      }
      cx.notify();
      return;
    }
    self.show_workbench = true;
    self.tasks.update(cx, |tasks, cx| {
      tasks.recovery_tasks_only = true;
      tasks.refresh_tasks(String::new());
      cx.notify();
    });
    crate::workbench::select_tasks(&self.dock, window, cx);
    cx.notify();
  }
}
