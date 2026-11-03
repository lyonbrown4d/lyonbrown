//! Explicit user attachment drafts and one-shot image submission.
use crate::AgentClient;
use agent_media::{ImageAttachment, validate_images};
use agent_protocol::v1;
use anyhow::{Result, ensure};

#[derive(Clone, Debug)]
pub struct NamedImage {
  pub name: String,
  pub image: ImageAttachment,
}
#[derive(Clone, Default, Debug)]
pub struct ImageDraft {
  pub attachments: Vec<NamedImage>,
  /// Kept after transport failure; retry reuses exactly the same body and ID.
  pub pending: Option<ImageSubmission>,
}
#[derive(Clone)]
pub struct ImageSubmission {
  pub request_id: String,
  pub session_id: Option<String>,
  pub prompt: String,
  pub images: Vec<ImageAttachment>,
}
impl std::fmt::Debug for ImageSubmission {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("ImageSubmission")
      .field("request_id", &self.request_id)
      .field("session_id", &self.session_id)
      .field("images", &self.images.len())
      .finish_non_exhaustive()
  }
}
impl ImageDraft {
  pub fn add(&mut self, name: String, image: ImageAttachment) -> Result<()> {
    ensure!(
      self.pending.is_none(),
      "an unconfirmed image request is retained; inspect or retry it before changing attachments"
    );
    let mut images = self
      .attachments
      .iter()
      .map(|entry| entry.image.clone())
      .collect::<Vec<_>>();
    images.push(image.clone());
    validate_images("", &images)?;
    self.attachments.push(NamedImage { name, image });
    Ok(())
  }
  pub fn remove(&mut self, index: usize) -> Result<()> {
    ensure!(
      self.pending.is_none(),
      "an unconfirmed image request is retained; inspect or retry it before changing attachments"
    );
    ensure!(
      index < self.attachments.len(),
      "attachment index does not exist"
    );
    self.attachments.remove(index);
    Ok(())
  }
  pub fn prepare(
    &mut self,
    session: Option<&str>,
    prompt: &str,
    request_id: String,
  ) -> Result<ImageSubmission> {
    if let Some(pending) = &mut self.pending {
      ensure!(
        pending.prompt == prompt && pending.session_id.as_deref() == session,
        "restore the unconfirmed request's session and prompt before retrying"
      );
      return Ok(pending.clone());
    }
    let images = self
      .attachments
      .iter()
      .map(|entry| entry.image.clone())
      .collect::<Vec<_>>();
    ensure!(!images.is_empty(), "no images attached");
    validate_images(prompt, &images)?;
    let pending = ImageSubmission {
      request_id,
      session_id: session.map(String::from),
      prompt: prompt.into(),
      images,
    };
    self.pending = Some(pending.clone());
    Ok(pending)
  }
  pub fn bind_created_session(&mut self, session: &str) {
    if let Some(pending) = &mut self.pending
      && pending.session_id.is_none()
    {
      pending.session_id = Some(session.into());
    }
  }
  /// A definite server rejection happened before admission. Keep the selected
  /// images editable; ambiguous transport/storage outcomes must not call this.
  pub fn reject(&mut self, request_id: &str) {
    if self
      .pending
      .as_ref()
      .is_some_and(|pending| pending.request_id == request_id)
    {
      self.pending = None;
    }
  }
  pub fn acknowledge(&mut self, request_id: &str) -> bool {
    if self
      .pending
      .as_ref()
      .is_some_and(|pending| pending.request_id == request_id)
    {
      self.pending = None;
      self.attachments.clear();
      true
    } else {
      false
    }
  }
}
impl AgentClient {
  /// Do not retry automatically. The caller retains attachments and this request
  /// identity until the daemon confirms the same accepted Run.
  pub async fn start_run_with_attachments_owned(
    &self,
    session_id: &str,
    owner_epoch: u64,
    prompt: &str,
    request_id: &str,
    images: &[ImageAttachment],
  ) -> Result<v1::Run> {
    validate_images(prompt, images)?;
    let node = self.node().await?;
    ensure!(
      node.capabilities.iter().any(|cap| cap == "runs.images"),
      "daemon does not support explicit image inputs"
    );
    Ok(
      self
        .inner
        .clone()
        .start_run_with_attachments(v1::StartRunWithAttachmentsRequest {
          session_id: session_id.into(),
          owner_epoch,
          prompt: prompt.into(),
          idempotency_key: request_id.into(),
          images: images
            .iter()
            .map(|image| v1::RunImageAttachment {
              mime_type: image.mime_type.clone(),
              data: image.data.clone(),
              sha256: image.sha256.clone(),
            })
            .collect(),
        })
        .await?
        .into_inner(),
    )
  }
}

/// Recognize only the server's explicit pre-admission model capability refusal.
/// All ambiguous failures keep the original request and its exact attachments.
pub fn admission_was_rejected(error: &anyhow::Error) -> bool {
  error.downcast_ref::<tonic::Status>().is_some_and(|status| {
    status.code() == tonic::Code::FailedPrecondition
      && status.message()
        == "selected model does not support image input; attachments were not accepted"
  })
}
