use super::super::state::Workbench;
use super::operations::TaskOperation;
use gpui_kit::component::{
  ActiveTheme, Disableable, Sizable,
  button::Button,
  input::{Input, Textarea},
  select::Select,
};
use gpui_kit::{TestSupportExt as _, *};

impl Workbench {
  pub(crate) fn tasks_view(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    let selected = self.session.is_some();
    let active_parent = self.runs.iter().any(|run| run.status == "running");
    let blocked = self.task_pending || self.connection_pending;
    let mut content = div().id("task-list").test_support().size_full().overflow_y_scroll().p_4().flex().flex_col().gap_3()
      .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Tasks are durable in agentd. Worker attempt completion is separate from Master acceptance."))
      .child(Button::new("task-recovery-filter").small().label(if self.recovery_tasks_only { "查看全部 Task" } else { "仅查看阻塞恢复的 Task" }).disabled(blocked || !selected).on_click(cx.listener(|this, _, _, cx| {
        this.recovery_tasks_only = !this.recovery_tasks_only;
        this.refresh_tasks(String::new());
        cx.notify();
      })))
      .child(div().text_xs().child(if self.recovery_tasks_only {
        "正在核对未终结 Task。使用原 Worker 地址核对，再取消或明确采纳／拒绝。空页可能仍有下一页；完成后返回恢复面板刷新核对。"
      } else { "未终结 Task 会阻止原会话中的异常 Run 恢复。" }))
      .child(Input::new(&self.task_target_id).id("task-worker-id").disabled(blocked))
      .child(Input::new(&self.task_target_endpoint).id("task-worker-endpoint").disabled(blocked))
      .child(Select::new(&self.task_capabilities).id("task-capabilities").placeholder("Task permission scope").accessibility_label("Task permission scope").disabled(blocked))
      .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Deadline: 1 hour. Write access remains subject to Worker approval."))
      .child(
        div()
          .id("task-instructions")
          .test_support()
          .w_full()
          .h(px(84.))
          .child(
            Textarea::new(&self.task_instructions)
              .accessibility_id("task-instructions-editor")
              .aria_label("Task instructions")
              .disabled(blocked),
          )
          .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
            this.task_instructions.read(cx).focus_handle(cx).focus(window, cx);
          })),
      )
      .child(Button::new("task-dispatch").small().label(if self.dispatch_intent.is_some() { "Resolve original task first" } else { "Dispatch to Worker" }).disabled(blocked || !selected || !active_parent || self.dispatch_intent.is_some()).on_click(cx.listener(|this, _, _, cx| this.dispatch_task(cx))))
      .child(Button::new("task-refresh").small().label("First page").disabled(blocked || !selected).on_click(cx.listener(|this, _, _, _| this.refresh_tasks(String::new()))))
      .child(Button::new("task-next-page").small().label("Next page").disabled(blocked || !selected || self.task_cursor.is_empty()).on_click(cx.listener(|this, _, _, _| { let cursor = this.task_cursor.clone(); this.refresh_tasks(cursor); })))
      .child(div().text_xs().child(format!("Next cursor: {}", if self.task_cursor.is_empty() { "end of list" } else { &self.task_cursor })));
    if let Some(intent) = self.dispatch_intent.clone() {
      content = content
        .child(
          div()
            .id("task-dispatch-intent")
            .test_support()
            .text_xs()
            .child(format!(
              "Dispatch identity: task {} · attempt {} · idempotency {}",
              intent.task_id, intent.attempt_id, intent.idempotency_key
            )),
        )
        .child(div().text_xs().child(if intent.uncertain { "Outcome uncertain. Keep using this task ID until inspection or reconciliation confirms a state." } else { "Preflight failed before dispatch. Correct the form or explicitly start a separate task." }))
        .child(
          div()
            .flex()
            .gap_2()
            .child(
              Button::new("task-intent-inspect")
                .small()
                .label("Inspect original ID")
                .disabled(blocked)
                .on_click(cx.listener({
                  let id = intent.task_id.clone();
                  move |this, _, _, cx| this.operate_task(id.clone(), TaskOperation::Inspect, cx)
                })),
            )
            .child(
              Button::new("task-intent-reconcile")
                .small()
                .label("Reconcile original ID")
                .disabled(blocked)
                .on_click(cx.listener(move |this, _, _, cx| {
                  this.operate_task(intent.task_id.clone(), TaskOperation::Reconcile, cx)
                })),
            ),
        )
        .child(Button::new("task-dispatch-separate").small().label("Start separate task, retain original ID").disabled(blocked || !selected || !active_parent).on_click(cx.listener(|this, _, _, cx| this.dispatch_separate_task(cx))));
    }
    for (index, intent) in self.dispatch_history.iter().cloned().enumerate() {
      content = content
        .child(
          div()
            .id(SharedString::from(format!("task-dispatch-history-{index}")))
            .test_support()
            .text_xs()
            .child(format!(
              "Previous dispatch identity retained: {} · attempt {} · {}",
              intent.task_id,
              intent.attempt_id,
              if intent.uncertain {
                "uncertain"
              } else {
                "not sent"
              }
            )),
        )
        .child(
          div()
            .flex()
            .gap_2()
            .child(
              Button::new(SharedString::from(format!("task-history-inspect-{index}")))
                .small()
                .label("Inspect retained ID")
                .disabled(blocked)
                .on_click(cx.listener({
                  let id = intent.task_id.clone();
                  move |this, _, _, cx| this.operate_task(id.clone(), TaskOperation::Inspect, cx)
                })),
            )
            .child(
              Button::new(SharedString::from(format!(
                "task-history-reconcile-{index}"
              )))
              .small()
              .label("Reconcile retained ID")
              .disabled(blocked)
              .on_click(cx.listener(move |this, _, _, cx| {
                this.operate_task(intent.task_id.clone(), TaskOperation::Reconcile, cx)
              })),
            ),
        );
    }
    for task in self.tasks.clone() {
      let id = task
        .envelope
        .as_ref()
        .map(|value| value.task_id.clone())
        .unwrap_or_default();
      let task_status = task.status.clone();
      let attempt_status = task.attempt_status.clone();
      let result = task.result.clone();
      let detail = task.envelope.as_ref().map(|envelope| format!(
        "Parent run: {} · Session: {} · Owner: {} @ epoch {}\nWorkspace: {}\nInstructions: {}\nCapabilities: {}\nAttempt: {} · Idempotency: {}\nCursor: {} · Error: {}",
        envelope.parent_run_id, envelope.session_id, envelope.owner_node_id, envelope.owner_epoch,
        envelope.workspace, envelope.instructions, envelope.capabilities.join(", "), envelope.attempt_id,
        envelope.idempotency_key, task.cursor, task.error
      )).unwrap_or_else(|| format!("Cursor: {} · Error: {}", task.cursor, task.error));
      let row = div()
        .id(SharedString::from(format!("task-row-{id}")))
        .test_support()
        .p_2()
        .w_full()
        .min_w_0()
        .border_1()
        .rounded_md()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_xs().child(format!(
          "Task {id} · {task_status} · Worker attempt {attempt_status}"
        )))
        .child(div().text_sm().child(result))
        .child(div().text_xs().child(detail))
        .child(
          div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
              Button::new(SharedString::from(format!("task-inspect-{id}")))
                .small()
                .label("Inspect")
                .disabled(blocked || id.is_empty())
                .on_click(cx.listener({
                  let id = id.clone();
                  move |this, _, _, cx| this.operate_task(id.clone(), TaskOperation::Inspect, cx)
                })),
            )
            .child(
              Button::new(SharedString::from(format!("task-reconcile-{id}")))
                .small()
                .label("Reconcile")
                .disabled(blocked || id.is_empty())
                .on_click(cx.listener({
                  let id = id.clone();
                  move |this, _, _, cx| this.operate_task(id.clone(), TaskOperation::Reconcile, cx)
                })),
            )
            .child(
              Button::new(SharedString::from(format!("task-cancel-{id}")))
                .small()
                .label("Cancel")
                .disabled(
                  blocked
                    || id.is_empty()
                    || !matches!(
                      attempt_status.as_str(),
                      "accepted" | "running" | "uncertain"
                    ),
                )
                .on_click(cx.listener({
                  let id = id.clone();
                  move |this, _, _, cx| this.operate_task(id.clone(), TaskOperation::Cancel, cx)
                })),
            )
            .child(
              Button::new(SharedString::from(format!("task-accept-{id}")))
                .small()
                .label("Accept result")
                .disabled(
                  blocked
                    || id.is_empty()
                    || task_status != "awaiting_acceptance"
                    || attempt_status != "succeeded",
                )
                .on_click(cx.listener({
                  let id = id.clone();
                  move |this, _, _, cx| {
                    this.operate_task(id.clone(), TaskOperation::Accept(true), cx)
                  }
                })),
            )
            .child(
              Button::new(SharedString::from(format!("task-reject-{id}")))
                .small()
                .label("Reject result")
                .disabled(
                  blocked
                    || id.is_empty()
                    || task_status != "awaiting_acceptance"
                    || attempt_status != "succeeded",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                  this.operate_task(id.clone(), TaskOperation::Accept(false), cx)
                })),
            ),
        );
      content = content.child(row);
    }
    if self.tasks.is_empty() && self.recovery_tasks_only {
      content = content.child(div().text_xs().child(if self.task_pending {
        "正在读取未终结 Task…"
      } else {
        "当前已加载页没有未终结 Task。如有下一页，请继续查询；恢复条件须返回恢复面板刷新核对。"
      }));
    }
    if let Some(error) = &self.error {
      content = content.child(
        div()
          .id("task-error")
          .test_support()
          .text_xs()
          .text_color(cx.theme().danger)
          .child(error.clone()),
      );
    }
    content.into_any_element()
  }
}
