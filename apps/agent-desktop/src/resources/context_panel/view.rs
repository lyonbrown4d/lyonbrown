use super::{Command, ContextPanel};
use agent_protocol::v1;
use gpui_kit::{
  component::{Disableable, Sizable, button::Button},
  *,
};
impl Render for ContextPanel {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let blocked = self.pending || self.client.is_none() || self.session.is_none();
    let mut page = div()
      .flex()
      .flex_col()
      .gap_2()
      .mt_4()
      .child("Context and execution mode");
    if let Some(context) = &self.context {
      page = page
        .child(format!(
          "Estimated input: {} tokens · window {} · output reserve {} (heuristic; idle inspection excludes tool schemas)",
          context.estimated_input_tokens, context.context_window, context.reserved_output_tokens
        ))
        .child(format!(
          "{} messages · {} compactions · revision {} · pending summary: {}",
          context.message_count,
          context.compaction_count,
          context.revision,
          context.summary_request_pending
        ));
    }
    if let Some(job) = self.maintenance.as_ref() {
      page = page.child(format!(
        "Context operation: {} · {:?}",
        job.id,
        v1::ContextMaintenanceState::try_from(job.state).ok()
      ));
      if job.state == v1::ContextMaintenanceState::Uncertain as i32 {
        page = page.child("The outcome is unknown. Closing preserves this context and prevents further execution in this session.")
          .child(Button::new("context-abandon").small().label("Close uncertain session without replay").disabled(blocked)
            .on_click(cx.listener(|this, _, _, cx| this.dispatch(Command::Abandon, cx))));
      }
      if job.state == v1::ContextMaintenanceState::Abandoned as i32 {
        page = page
          .child("This context is retained for inspection. Start a new chat to continue.")
          .child(
            Button::new("context-new-chat")
              .small()
              .label("New chat")
              .disabled(blocked)
              .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::shell::NewChatTab), cx)
              }),
          );
      }
      if matches!(
        v1::ContextMaintenanceState::try_from(job.state).ok(),
        Some(v1::ContextMaintenanceState::Preparing | v1::ContextMaintenanceState::Sending)
      ) {
        page = page.child(
          Button::new("context-cancel")
            .small()
            .label("Cancel compaction")
            .disabled(blocked || job.cancel_requested)
            .on_click(cx.listener(|this, _, _, cx| this.dispatch(Command::Cancel, cx))),
        );
      }
    }
    let mut actions = div().flex().flex_wrap().gap_2();
    for (id, label, command, unavailable) in [
      ("context-refresh", "Refresh", Command::Refresh, false),
      (
        "context-summary",
        "Inspect summary",
        Command::Summary,
        false,
      ),
      (
        "context-compact",
        "Compact context",
        Command::Compact,
        !self.context.as_ref().is_some_and(|c| {
          c.summary_pipeline_available
            && c.manual_operations_available
            && !c.summary_request_pending
        }),
      ),
      (
        "context-undo",
        "Undo compaction",
        Command::Undo,
        !self.context.as_ref().is_some_and(|c| c.can_undo),
      ),
    ] {
      actions = actions.child(
        Button::new(id)
          .small()
          .label(label)
          .disabled(blocked || unavailable)
          .on_click(cx.listener(move |this, _, _, cx| this.dispatch(command, cx))),
      );
    }
    page = page.child(actions);
    if let Some(policy) = &self.policy {
      page = page.child(format!(
        "Mode: {:?} · revision {}. Existing permissions and approvals still apply.",
        v1::InteractionMode::try_from(policy.mode).ok(),
        policy.revision
      ));
      let mut modes = div().flex().gap_2();
      for (id, label, mode) in [
        ("mode-read", "Read only", v1::InteractionMode::ReadOnly),
        ("mode-plan", "Plan", v1::InteractionMode::Plan),
        ("mode-execute", "Execute", v1::InteractionMode::Execute),
      ] {
        modes = modes.child(
          Button::new(id)
            .small()
            .label(label)
            .disabled(blocked || policy.mode == mode as i32)
            .on_click(cx.listener(move |this, _, _, cx| this.dispatch(Command::Mode(mode), cx))),
        );
      }
      page = page.child(modes);
      for tool in &policy.tools {
        page = page.child(format!(
          "{} · {}{}",
          tool.name,
          agent_client::tool_availability_label(tool),
          if tool.requires_approval {
            " · approval required"
          } else {
            ""
          }
        ));
      }
    }
    if !self.summary.is_empty() {
      page = page.child(self.summary.clone());
    }
    page.child(self.message.clone())
  }
}
