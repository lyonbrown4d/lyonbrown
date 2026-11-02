use super::*;
use gpui_kit::TestSupportExt as _;
use gpui_kit::component::{Disableable, Sizable, button::Button, input::Textarea};

impl Desktop {
  pub(crate) fn recovery_view(&self, cx: &mut Context<Self>) -> AnyElement {
    let Some(state) = &self.vm.recovery else {
      return div().into_any_element();
    };
    let blocked = state.pending || self.navigation.connecting || self.persistence_loading;
    let mut content = div()
      .id("run-recovery")
      .test_support()
      .max_h(px(460.))
      .overflow_y_scroll()
      .p_3()
      .flex()
      .flex_col()
      .gap_2()
      .child(
        div()
          .text_sm()
          .child(format!("异常运行核对 · {}", state.run_id)),
      )
      .child(
        div()
          .text_xs()
          .child("先核对实际副作用。原运行保留 Uncertain，旧动作不会重放。核对记录用于留档；新会话的模型仅收到新输入，请写明核对结果。"),
      )
      .child(
        div()
          .flex()
          .flex_wrap()
          .gap_2()
          .child(
            Button::new("recovery-refresh")
              .small()
              .label("刷新核对")
              .disabled(blocked)
              .on_click(cx.listener(|this, _, _, cx| this.inspect_recovery(cx))),
          )
          .child(
            Button::new("recovery-tasks")
              .small()
              .label("核对未终结 Task")
              .disabled(blocked || self.tasks.read(cx).task_pending)
              .on_click(cx.listener(|this, _, window, cx| this.open_recovery_tasks(window, cx))),
          )
          .child(
            Button::new("recovery-close")
              .small()
              .label("返回对话")
              .disabled(blocked)
              .on_click(cx.listener(|this, _, window, cx| {
                // Hiding the form does not discard a possibly dispatched request.
                if let Some(state) = &mut this.vm.recovery {
                  state.visible = false;
                }
                this.composer.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
              })),
          ),
      );
    if let Some(report) = &state.report {
      content = content.child(div().text_xs().child(report.reason.clone()));
      for tool in &report.pending_tools {
        content = content.child(div().text_xs().child(format!("待核对工具：{tool}")));
      }
      if let Some(result) = &report.resolution {
        content = content.child(
          div()
            .id("recovery-resolution")
            .test_support()
            .text_xs()
            .child(format!(
              "已记录决定 · {}\n核对记录：{}",
              if result.action == v1::RunRecoveryAction::Abandon as i32 {
                "放弃原运行"
              } else {
                "新会话继续"
              },
              result.note
            )),
        );
        if let Some(session) = &result.session {
          let id = session.id.clone();
          content = content.child(
            Button::new("recovery-open-session")
              .small()
              .label("打开继续会话")
              .disabled(blocked)
              .on_click(
                cx.listener(move |this, _, window, cx| this.select_session(id.clone(), window, cx)),
              ),
          );
        }
      } else {
        let retained = state.intent.is_some();
        content = content
          .child(div().text_xs().child("核对记录"))
          .child(recovery_input(
            "recovery-note",
            "核对记录",
            &state.note,
            blocked || retained,
            cx,
          ))
          .child(div().text_xs().child("新会话输入（仅继续时使用）"))
          .child(recovery_input(
            "recovery-prompt",
            "新会话输入",
            &state.prompt,
            blocked || retained,
            cx,
          ));
        if let Some(intent) = &state.intent {
          content = content.child(div().text_xs().child(format!(
            "保留的请求身份：{}。刷新核对后才能使用相同内容重试。",
            intent.idempotency_key
          )));
        }
        let mut actions = div().flex().flex_wrap().gap_2();
        for (id, action, label) in [
          (
            "recovery-abandon",
            v1::RunRecoveryAction::Abandon,
            "记录放弃原运行",
          ),
          (
            "recovery-continue",
            v1::RunRecoveryAction::ContinueNewSession,
            "按新输入继续",
          ),
        ] {
          let enabled = !blocked
            && state.inspected
            && report.allowed_actions.contains(&(action as i32))
            && state
              .intent
              .as_ref()
              .is_none_or(|intent| intent.action == action as i32);
          actions = actions.child(
            Button::new(id)
              .small()
              .label(if retained {
                format!("原身份重试：{label}")
              } else {
                label.into()
              })
              .disabled(!enabled)
              .on_click(cx.listener(move |this, _, _, cx| this.resolve_recovery(action, cx))),
          );
        }
        content = content.child(actions);
        if report.allowed_actions.is_empty() {
          content = content.child(div().text_xs().child("当前不能恢复。请先核对并结束原会话中的所有未终结 Task，确认运行已停止及 Master 所有权，再刷新核对。"));
        }
      }
    }
    if let Some(error) = &state.error {
      content = content.child(
        div()
          .id("recovery-error")
          .test_support()
          .text_xs()
          .child(error.clone()),
      );
    }
    content.into_any_element()
  }
}

fn recovery_input(
  id: &'static str,
  label: &'static str,
  input: &Entity<TextareaState>,
  disabled: bool,
  cx: &mut Context<Desktop>,
) -> AnyElement {
  let focus = input.clone();
  div()
    .id(id)
    .test_support()
    .h(px(56.))
    .w_full()
    .child(
      Textarea::new(input)
        .accessibility_id(format!("{id}-input"))
        .aria_label(label)
        .h(px(56.))
        .disabled(disabled),
    )
    .on_mouse_down(
      MouseButton::Left,
      cx.listener(move |_, _, window, cx| {
        if !disabled {
          focus.read(cx).focus_handle(cx).focus(window, cx);
        }
      }),
    )
    .into_any_element()
}
