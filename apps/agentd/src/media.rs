//! Authenticated user image admission. No image discovery or screen capture.
use crate::Daemon;
use agent_core::Run;
use agent_media::ImageAttachment;
use agent_protocol::v1;
use agent_runtime::RuntimeImage;
use base64::Engine;
use tonic::Status;

impl Daemon {
  pub(crate) async fn start_attached_run(
    &self,
    input: v1::StartRunWithAttachmentsRequest,
  ) -> Result<v1::Run, Status> {
    if uuid::Uuid::parse_str(&input.idempotency_key).is_err() {
      return Err(Status::invalid_argument(
        "image input requires a UUID request identity",
      ));
    }
    let _submission = self.submission.lock().await;
    if self.stopping.is_cancelled() || self.draining.load(std::sync::atomic::Ordering::Acquire) {
      return Err(Status::unavailable("daemon is shutting down"));
    }
    let session = self
      .store
      .session(&input.session_id)
      .await
      .map_err(crate::service::db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id || session.owner_epoch != input.owner_epoch {
      return Err(Status::failed_precondition(
        "session owner or epoch changed",
      ));
    }
    let images = input
      .images
      .into_iter()
      .map(|image| ImageAttachment {
        mime_type: image.mime_type,
        data: image.data,
        sha256: image.sha256,
      })
      .collect::<Vec<_>>();
    agent_media::validate_images(&input.prompt, &images).map_err(|_| {
      Status::invalid_argument(
        "image input exceeds supported count, encoding, digest or pixel limits",
      )
    })?;
    let prompt = self
      .protect_private_text(&session.id, input.owner_epoch, &input.prompt)
      .await?;
    if let Some(run) = self
      .store
      .accepted_run_with_media(
        &input.session_id,
        input.owner_epoch,
        &prompt,
        &input.idempotency_key,
        &images,
      )
      .await
      .map_err(crate::service::db_error)?
    {
      return Ok(run.into());
    }
    if self.active.lock().await.len() >= 128 {
      return Err(Status::resource_exhausted("node run capacity reached"));
    }
    if !images.is_empty() {
      let configured = self
        .configured_for_run(&session.id)
        .await
        .map_err(crate::service::db_error)?;
      let mut config = configured.config.runtime(&session);
      configured
        .normalize_runtime_limits(&mut config)
        .map_err(crate::service::db_error)?;
      if !config.supports_images {
        return Err(Status::failed_precondition(
          "selected model does not support image input; attachments were not accepted",
        ));
      }
    }
    let (run, created) = self
      .store
      .accept_run_with_media(
        &input.session_id,
        input.owner_epoch,
        &prompt,
        &input.idempotency_key,
        &images,
      )
      .await
      .map_err(crate::service::db_error)?;
    if created {
      self
        .launch(run.clone())
        .await
        .map_err(crate::service::db_error)?;
    }
    self.events.notify_waiters();
    Ok(run.into())
  }
  pub(crate) async fn media_input(&self, run: &Run) -> anyhow::Result<Vec<RuntimeImage>> {
    let images = self.store.run_media(&run.id).await?;
    agent_media::validate_images(&run.prompt, &images)?;
    Ok(
      images
        .into_iter()
        .map(|image| RuntimeImage {
          mime_type: image.mime_type,
          data_base64: base64::engine::general_purpose::STANDARD.encode(image.data),
        })
        .collect(),
    )
  }
}
