use super::watch::watch;
use crate::{
  connection::{Update, public_error},
  shell::Desktop,
};
use gpui_kit::*;
impl Desktop {
  pub(crate) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    if self.router.is_settings() {
      return;
    }
    if self
      .navigation
      .unbound
      .contains_key(&self.navigation.active)
    {
      self.vm.error = Some(
        "This restored draft has no session. Create a session for this draft before submitting."
          .into(),
      );
      cx.notify();
      return;
    }
    if self.composer.update(cx, |input, cx| {
      input.marked_text_range(window, cx).is_some()
    }) {
      return;
    }
    if self.navigation.connecting
      || self.persistence_loading
      || self.vm.busy
      || self.vm.active_run.is_some()
    {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    let prompt = self.composer.read(cx).value().to_string();
    if prompt.trim().is_empty() && self.vm.images.attachments.is_empty() {
      return;
    }
    let session = self.vm.selected.clone();
    let media = if self.vm.images.attachments.is_empty() {
      None
    } else {
      match self.vm.images.prepare(
        session.as_deref(),
        &prompt,
        uuid::Uuid::now_v7().to_string(),
      ) {
        Ok(submission) => Some(submission),
        Err(error) => {
          self.vm.error = Some(error.to_string());
          cx.notify();
          return;
        }
      }
    };
    self.vm.busy = true;
    self.navigation.pending_requests += 1;
    self.vm.error = None;
    let workspace = self.workspace.read(cx).value().to_string();
    let sender = self.scoped_sender();
    let key = uuid::Uuid::now_v7().to_string();
    self.runtime.spawn(async move {
      let result = async {
        let session_id = match session {
          Some(id) => id,
          None => {
            let title = if prompt.trim().is_empty() {
              "Image task".into()
            } else {
              prompt.chars().take(48).collect::<String>()
            };
            let session = client.create_session(&title, &workspace).await?;
            let id = session.id.clone();
            let _ = sender.send(Update::Session(session, true)).await;
            id
          }
        };
        let run = if let Some(media) = media {
          let epoch = client
            .runtime_status(&session_id)
            .await?
            .session
            .ok_or_else(|| anyhow::anyhow!("session unavailable"))?
            .owner_epoch;
          let result = client
            .start_run_with_attachments_owned(
              &session_id,
              epoch,
              &prompt,
              &media.request_id,
              &media.images,
            )
            .await;
          let run = match result {
            Ok(run) => run,
            Err(error) => {
              if agent_client::media::admission_was_rejected(&error) {
                sender
                  .send(Update::ImagesRejected(media.request_id.clone()))
                  .await;
              }
              return Err(error);
            }
          };
          let _ = sender.send(Update::ImagesConfirmed(media.request_id)).await;
          run
        } else {
          client.start_run(&session_id, &prompt, &key).await?
        };
        let id = run.id.clone();
        let _ = sender.send(Update::Started(run)).await;
        let watcher = sender.clone();
        sender
          .attach_watch(tokio::spawn(async move {
            if let Err(error) = watch(client, id, watcher.clone(), 0).await {
              let _ = watcher.send(Update::Error(public_error(error))).await;
            }
          }))
          .await;
        anyhow::Ok(())
      }
      .await;
      if let Err(error) = result {
        let _ = sender
          .send(Update::Error(format!(
            "{} No request was replayed; inspect the run before retrying.",
            public_error(error)
          )))
          .await;
      }
      sender.finish_request().await;
    });
    cx.notify();
  }
}
