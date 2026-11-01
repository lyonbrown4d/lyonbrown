use super::state::{Reply, ResourcePane};
use gpui_kit::component::{
  ActiveTheme, Disableable, Sizable, button::Button, input::Input, select::Select,
};
use gpui_kit::*;

impl ResourcePane {
  pub(super) fn inspect_skill(&mut self, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let source = self.source.read(cx).value().to_string();
    if source.trim().is_empty() {
      return;
    }
    let scope = self.scope_name(cx);
    let session = self.session.clone().unwrap_or_default();
    self.skill_receipt = None;
    self.request(
      false,
      async move {
        let inspection = client.inspect_skill(&source, &scope, &session).await?;
        Ok(Reply::InspectedSkill(source, scope, inspection))
      },
      cx,
    );
  }
  pub(super) fn install_skill(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some((source, scope, inspection))) =
      (self.client.clone(), self.skill_receipt.clone())
    else {
      return;
    };
    if source != self.source.read(cx).value().as_ref() || scope != self.scope_name(cx) {
      return;
    }
    let Some(skill) = inspection.skill else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    self.skill_receipt = None;
    self.request(
      true,
      async move {
        client
          .install_skill(&source, &scope, &session, &skill.content_sha256)
          .await?;
        Ok(Reply::Changed("Inspected skill installed".into()))
      },
      cx,
    );
  }
  fn remove_skill(&mut self, skill: agent_protocol::v1::Skill, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    if !self.skills.iter().any(|listed| {
      listed.name == skill.name
        && listed.scope == skill.scope
        && listed.content_sha256 == skill.content_sha256
    }) {
      return;
    }
    let session = self.session.clone().unwrap_or_default();
    self.request(
      true,
      async move {
        client
          .remove_skill(&skill.name, &skill.scope, &session, &skill.content_sha256)
          .await?;
        Ok(Reply::Changed("Listed skill revision removed".into()))
      },
      cx,
    );
  }
  pub(super) fn skills_view(&self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    let receipt = self.skill_receipt.as_ref().filter(|(source, scope, _)| {
      source == self.source.read(cx).value().as_ref() && *scope == self.scope_name(cx)
    });
    let mut page = div().flex().flex_col().gap_3().child("Skills")
      .child("Sources are resolved and installed by agentd. Inspect files and their hashes before confirming installation.")
      .child(Select::new(&self.scope).title_prefix("Scope"))
      .child(Input::new(&self.source).id("skill-source"))
      .child(div().flex().gap_2()
        .child(Button::new("skill-inspect").small().label("Inspect source").disabled(self.busy || !self.ready() || self.pending_mutations > 0)
          .on_click(cx.listener(|this, _, _, cx| this.inspect_skill(cx))))
        .child(Button::new("skill-install").small().label("Confirm inspected installation").disabled(self.busy || receipt.is_none() || self.pending_mutations > 0)
          .on_click(cx.listener(|this, _, _, cx| this.install_skill(cx)))));
    if let Some((_, _, inspection)) = receipt {
      if let Some(skill) = &inspection.skill {
        page = page.child(format!(
          "{} · {} files · {} bytes · SHA256 {}",
          skill.name, skill.files, skill.total_bytes, skill.content_sha256
        ));
      }
      page = page.children(inspection.files.iter().map(|file| {
        div().text_xs().child(format!(
          "{} · {} bytes · {}",
          file.path, file.bytes, file.sha256
        ))
      }));
    }
    page
      .children(self.skills.iter().enumerate().map(|(index, skill)| {
        let listed = skill.clone();
        div()
          .p_3()
          .border_1()
          .border_color(cx.theme().border)
          .rounded_md()
          .child(format!(
            "{} · {} · {} files · {} bytes",
            skill.name, skill.scope, skill.files, skill.total_bytes
          ))
          .child(skill.description.clone())
          .child(
            div()
              .text_xs()
              .child(format!("SHA256 {}", skill.content_sha256)),
          )
          .child(
            Button::new(("skill-remove", index))
              .small()
              .label("Remove this revision")
              .disabled(self.busy || self.pending_mutations > 0)
              .on_click(cx.listener(move |this, _, _, cx| this.remove_skill(listed.clone(), cx))),
          )
      }))
      .into_any_element()
  }
}
