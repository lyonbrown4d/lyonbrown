use super::WorkflowPane;
use agent_protocol::v1;
use gpui_kit::{
  component::{ActiveTheme, Disableable, Sizable, button::Button, input::Input, text::TextView},
  *,
};
impl Render for WorkflowPane {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let blocked = self.busy || self.pending || self.client.is_none() || self.session.is_none();
    let mut page = div().id("workflow-panel").h(px(560.)).flex().flex_col().gap_3().overflow_y_scroll()
      .child("Plans and reviewed changes")
      .child(if self.session.is_some() { "Plans track progress. Model reports and host command receipts are shown separately; neither expands permissions." }
        else { "Select a chat to inspect its plans and checkpoints." })
      .child(Button::new("workflow-refresh").small().label(if self.busy { "Working…" } else { "Refresh" }).disabled(blocked)
        .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))))
      .child(self.message.clone())
      .child(div().font_weight(FontWeight::SEMIBOLD).child("Durable plans"))
      .children(self.plans.iter().enumerate().map(|(index, plan)| {
        let id = plan.id.clone();
        Button::new(("workflow-plan", index)).small().label(format!("{} · revision {} · {} steps", plan.title, plan.revision, plan.steps.len()))
          .disabled(blocked).on_click(cx.listener(move |this, _, _, cx| this.inspect(id.clone(), true, cx)))
      }));
    if self.plans.is_empty() && !self.busy {
      page = page.child("No plans in this result.");
    }
    if !self.plan_after.is_empty() {
      page = page.child(
        Button::new("workflow-more-plans")
          .small()
          .label("More plans")
          .disabled(blocked)
          .on_click(cx.listener(|this, _, _, cx| this.more(true, cx))),
      );
    }
    if let Some(plan) = &self.plan {
      page = page
        .child(format!(
          "{} · revision {} · {}",
          plan.title, plan.revision, plan.id
        ))
        .child(Input::new(&self.report).id("plan-report").disabled(blocked))
        .child(
          Input::new(&self.operation)
            .id("plan-operation")
            .disabled(blocked),
        );
      for (index, step) in plan.steps.iter().enumerate() {
        let Some(definition) = &step.definition else {
          continue;
        };
        let id = definition.id.clone();
        let verify_id = id.clone();
        let complete_id = id.clone();
        let state = v1::WorkflowStepState::try_from(step.state).ok();
        let active = matches!(
          state,
          Some(v1::WorkflowStepState::Running | v1::WorkflowStepState::Blocked)
        );
        page = page.child(
          div()
            .flex()
            .flex_col()
            .gap_1()
            .mt_2()
            .child(format!(
              "{:?} · {}",
              state.unwrap_or_default(),
              definition.title
            ))
            .child(format!("Acceptance: {}", definition.acceptance.join("; ")))
            .child(format!(
              "Dependencies: {}",
              definition.dependencies.join(", ")
            ))
            .child(format!("Model/user report: {}", step.reported_result))
            .child(format!(
              "Host receipt references: {}",
              step.verification_refs.join(", ")
            ))
            .children(
              step
                .user_override
                .iter()
                .map(|reason| div().child(format!("Explicit user completion: {reason}"))),
            )
            .child(
              div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                  Button::new(("plan-step-start", index))
                    .small()
                    .label("Start / resume")
                    .disabled(
                      blocked
                        || !matches!(
                          state,
                          Some(v1::WorkflowStepState::Pending | v1::WorkflowStepState::Blocked)
                        ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                      this.update_step(id.clone(), v1::WorkflowStepState::Running, cx)
                    })),
                )
                .child(
                  Button::new(("plan-step-verify", index))
                    .small()
                    .label("Record host verification")
                    .disabled(blocked || !active)
                    .on_click(
                      cx.listener(move |this, _, _, cx| this.verify(verify_id.clone(), cx)),
                    ),
                )
                .child(
                  Button::new(("plan-step-complete", index))
                    .small()
                    .label("Complete verified step / explain override")
                    .disabled(
                      blocked
                        || matches!(
                          state,
                          Some(v1::WorkflowStepState::Completed | v1::WorkflowStepState::Skipped)
                        ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                      this.update_step(complete_id.clone(), v1::WorkflowStepState::Completed, cx)
                    })),
                ),
            ),
        );
      }
      for receipt in &plan.verifications {
        page = page.child(format!(
          "Host receipt {} · passed={} · {}",
          receipt.id, receipt.passed, receipt.summary
        ));
      }
    }
    page = page
      .child(
        div()
          .mt_4()
          .font_weight(FontWeight::SEMIBOLD)
          .child("Create a plan"),
      )
      .child(Input::new(&self.title).id("plan-title").disabled(blocked))
      .child(
        Input::new(&self.step_title)
          .id("plan-first-step")
          .disabled(blocked),
      )
      .child(
        Input::new(&self.acceptance)
          .id("plan-acceptance")
          .disabled(blocked),
      )
      .child(
        Button::new("plan-create")
          .small()
          .label("Create plan with first step")
          .disabled(blocked)
          .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
      )
      .child(
        div()
          .mt_4()
          .font_weight(FontWeight::SEMIBOLD)
          .child("Reviewed checkpoints"),
      )
      .children(self.changes.iter().enumerate().map(|(index, change)| {
        let id = change.id.clone();
        Button::new(("workflow-change", index))
          .small()
          .label(format!(
            "{} · {:?} · {} files",
            change.id,
            v1::WorkflowChangeState::try_from(change.state)
              .ok()
              .unwrap_or_default(),
            change.files.len()
          ))
          .disabled(blocked)
          .on_click(cx.listener(move |this, _, _, cx| this.inspect(id.clone(), false, cx)))
      }));
    if self.changes.is_empty() && !self.busy {
      page = page.child("No checkpoints in this result.");
    }
    if !self.change_after.is_empty() {
      page = page.child(
        Button::new("workflow-more-changes")
          .small()
          .label("More checkpoints")
          .disabled(blocked)
          .on_click(cx.listener(|this, _, _, cx| this.more(false, cx))),
      );
    }
    if let Some(change) = &self.change {
      let state = v1::WorkflowChangeState::try_from(change.state).ok();
      page = page
        .child(format!(
          "{} · revision {} · {:?}",
          change.id,
          change.revision,
          state.unwrap_or_default()
        ))
        .child(format!("Exact preview hash: {}", change.preview_sha256))
        .child(TextView::new(&self.diff).selectable(true))
        .child(
          div()
            .flex()
            .gap_2()
            .child(
              Button::new("workflow-undo-preview")
                .small()
                .label("Build reverse preview")
                .disabled(blocked || state != Some(v1::WorkflowChangeState::Applied))
                .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
            )
            .child(
              Button::new("workflow-apply")
                .small()
                .label("Apply reviewed diff")
                .disabled(
                  blocked
                    || !change.detail_included
                    || state != Some(v1::WorkflowChangeState::Preview),
                )
                .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
            ),
        )
        .children(change.validations.iter().map(|receipt| {
          div().child(format!(
            "Historical command receipt {} · passed={} · exit {:?}",
            receipt.operation_id, receipt.passed, receipt.exit_code
          ))
        }));
    }
    page.text_color(cx.theme().foreground)
  }
}
