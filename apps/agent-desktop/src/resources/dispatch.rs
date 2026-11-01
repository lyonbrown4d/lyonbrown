use super::{
  ResourceSection,
  state::{Completion, Reply, ResourcePane},
};
use gpui_kit::*;

impl ResourcePane {
  pub(super) fn request<F>(&mut self, mutation: bool, future: F, cx: &mut Context<Self>)
  where
    F: std::future::Future<Output = anyhow::Result<Reply>> + Send + 'static,
  {
    if self.busy || !self.ready() || self.pending_mutations > 0 {
      return;
    }
    self.busy = true;
    self.error = None;
    self.notice = None;
    if mutation {
      self.pending_mutations += 1;
    }
    let sender = self.sender.clone();
    let generation = self.generation;
    let input_revision = self.input_revision;
    self.runtime.spawn(async move {
      let result = future.await;
      let _ = sender
        .send(Completion {
          generation,
          input_revision,
          mutation,
          result,
        })
        .await;
    });
    cx.notify();
  }
  pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    let section = self.section;
    let scope = self.scope_name(cx);
    self.request(
      false,
      async move {
        Ok(match section {
          ResourceSection::Models => Reply::Models(client.list_models(&session).await?),
          ResourceSection::Providers => Reply::Providers(client.list_providers().await?),
          ResourceSection::Skills => Reply::Skills(client.list_skills(&scope, &session).await?),
          ResourceSection::Status => {
            Reply::Status(Box::new(client.runtime_status(&session).await?))
          }
        })
      },
      cx,
    );
  }
  pub(super) fn apply(&mut self, result: anyhow::Result<Reply>, cx: &mut Context<Self>) {
    match result {
      Err(error) => self.error = Some(error.to_string()),
      Ok(reply) => match reply {
        Reply::Models(models) => self.models = models,
        Reply::Providers(providers) => self.providers = providers,
        Reply::Skills(skills) => self.skills = skills,
        Reply::Status(status) => self.status = Some(*status),
        Reply::InspectedSkill(source, scope, inspection) => {
          if self.source.read(cx).value().as_ref() == source && self.scope_name(cx) == scope {
            self.skill_receipt = Some((source, scope, inspection));
          }
        }
        Reply::Hub(request, value) => self.apply_hub(request, value, cx),
        Reply::Changed(message) => {
          self.refresh(cx);
          self.notice = Some(message);
        }
      },
    }
  }
  pub(super) fn select_model(&mut self, model: String, execution: String, cx: &mut Context<Self>) {
    if !self.models.iter().any(|candidate| {
      candidate.id == model
        && candidate
          .executions
          .iter()
          .any(|choice| choice.id == execution && choice.state == "ready")
    }) {
      return;
    }
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.request(
      true,
      async move {
        // Fetch ownership at submission time. A later ownership change is rejected by agentd CAS.
        let current = client
          .runtime_status(&session)
          .await?
          .session
          .ok_or_else(|| anyhow::anyhow!("Session no longer available"))?;
        anyhow::ensure!(current.id == session, "Session scope changed");
        client
          .select_model(&session, current.owner_epoch, &model, &execution)
          .await?;
        Ok(Reply::Changed(
          "Model selection saved to this session".into(),
        ))
      },
      cx,
    );
  }
}
