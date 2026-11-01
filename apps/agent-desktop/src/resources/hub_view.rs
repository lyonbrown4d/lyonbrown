use super::{hub::text, state::ResourcePane};
use gpui_kit::component::{Disableable, Sizable, button::Button, input::Input, select::Select};
use gpui_kit::{TestSupportExt as _, *};
use serde_json::Value;

impl ResourcePane {
  pub(super) fn hub_view(&self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    let disabled = self.busy || !self.ready() || self.pending_mutations > 0;
    let inspection = self
      .hub_inspection
      .as_ref()
      .filter(|(input, _)| self.same_source(input, cx));
    let plan = self.current_plan(cx);
    let mut page = div()
      .flex()
      .flex_col()
      .gap_2()
      .mt_4()
      .child("Find and install from Hugging Face")
      .child(Input::new(&self.query).id("model-hub-query"))
      .child(
        Button::new("model-hub-search")
          .small()
          .label("Search")
          .disabled(disabled)
          .on_click(cx.listener(|this, _, _, cx| this.hub_action("search", String::new(), cx))),
      )
      .children(self.hub_results.iter().enumerate().map(|(index, result)| {
        let repository = text(result, "repository");
        let revision = text(result, "revision");
        Button::new(("model-hub-result", index))
          .small()
          .label(repository.clone())
          .disabled(disabled)
          .on_click(cx.listener(move |this, _, window, cx| {
            this.repository.update(cx, |input, cx| {
              input.set_value(repository.clone(), window, cx)
            });
            this.revision.update(cx, |input, cx| {
              input.set_value(revision.clone(), window, cx)
            });
          }))
      }))
      .child(Input::new(&self.repository).id("model-hub-repository"))
      .child(Input::new(&self.revision).id("model-hub-revision"))
      .child(
        Button::new("model-hub-inspect")
          .small()
          .label("Inspect repository")
          .disabled(disabled)
          .on_click(cx.listener(|this, _, _, cx| this.hub_action("inspect", String::new(), cx))),
      );
    if let Some((_, inspected)) = inspection {
      page = page
        .child(format!(
          "Revision {} · License {} · Gated {}",
          text(inspected, "revision"),
          text(inspected, "license"),
          inspected.get("gated").is_some_and(|v| v == true)
        ))
        .children(
          inspected
            .get("files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|file| {
              div().text_xs().child(format!(
                "{} · {} bytes · {}",
                text(file, "path"),
                file.get("bytes").unwrap_or(&Value::Null),
                text(file, "sha256")
              ))
            }),
        );
    }
    page = page
      .child(Input::new(&self.file).id("model-hub-file"))
      .child(
        Button::new("model-hub-advanced")
          .small()
          .label("Advanced runtime options")
          .on_click(cx.listener(|this, _, _, cx| {
            this.advanced = !this.advanced;
            cx.notify();
          })),
      );
    if self.advanced {
      page = page.child(Select::new(&self.backend).title_prefix("Runtime: "));
    }
    page = page.child(
      Button::new("model-hub-plan")
        .small()
        .label("Review installation plan")
        .disabled(disabled || inspection.is_none())
        .on_click(cx.listener(|this, _, _, cx| this.hub_action("plan", String::new(), cx))),
    );
    if let Some((_, plan)) = plan {
      page = page
        .child(format!(
          "{} · {} bytes · {}",
          text(plan, "repository"),
          plan.get("total_bytes").unwrap_or(&Value::Null),
          text(plan, "compatibility")
        ))
        .child(
          div()
            .id("model-hub-plan-receipt")
            .test_support()
            .aria_label(text(plan, "id"))
            .text_xs()
            .child(format!(
              "Plan ID: {} · Revision: {}",
              text(plan, "id"),
              text(plan, "revision")
            )),
        )
        .child(
          Button::new("model-hub-install")
            .small()
            .label("Confirm exact plan and download")
            .disabled(disabled)
            .on_click(cx.listener(|this, _, _, cx| this.hub_action("install", String::new(), cx))),
        );
    }
    page = page.child(
      Button::new("model-hub-installed")
        .small()
        .label("Refresh installed artifacts")
        .disabled(disabled)
        .on_click(cx.listener(|this, _, _, cx| this.hub_action("installed", String::new(), cx))),
    );
    for (index, model) in self.installed.iter().enumerate() {
      let plan = model.get("plan").unwrap_or(&Value::Null);
      let id = text(plan, "id");
      let mut row = div().flex().flex_col().gap_2().child(format!(
        "{} · {}",
        text(plan, "repository"),
        text(model, "state")
      ));
      for (offset, action) in ["load", "unload", "remove"].into_iter().enumerate() {
        let id = id.clone();
        row = row.child(
          Button::new(("model-artifact-action", index * 3 + offset))
            .small()
            .label(action)
            .disabled(disabled)
            .on_click(cx.listener(move |this, _, _, cx| this.hub_action(action, id.clone(), cx))),
        );
      }
      page = page.child(row);
    }
    page = page
      .child(Input::new(&self.job_id).id("model-job-id"))
      .child(
        Button::new("model-job-status")
          .small()
          .label("Inspect saved job ID")
          .disabled(disabled)
          .on_click(cx.listener(|this, _, _, cx| {
            let id = this.job_id.read(cx).value().to_string();
            if !id.trim().is_empty() {
              this.hub_action("status", id, cx);
            }
          })),
      );
    for (index, (id, job)) in self.jobs.iter().enumerate() {
      let mut row = div()
        .id(("model-job", index))
        .test_support()
        .aria_label(format!("{} {}", id, text(job, "state")))
        .flex()
        .flex_col()
        .gap_2()
        .child(format!(
          "Job {} · {} · {}/{} bytes",
          id,
          text(job, "state"),
          job.get("downloaded_bytes").unwrap_or(&Value::Null),
          job.get("total_bytes").unwrap_or(&Value::Null)
        ));
      for (offset, action) in ["status", "cancel"].into_iter().enumerate() {
        let id = id.clone();
        row = row.child(
          Button::new(("model-job-action", index * 2 + offset))
            .small()
            .label(action)
            .disabled(disabled)
            .on_click(cx.listener(move |this, _, _, cx| this.hub_action(action, id.clone(), cx))),
        );
      }
      page = page.child(row);
    }
    page.into_any_element()
  }
}
