use super::{Reply, Request};
use crate::{
  backend::Update,
  session_backend::{refresh_session, watch},
};
use agent_client::AgentClient;
use tokio::sync::mpsc;
pub(crate) async fn execute(
  client: &AgentClient,
  request: Request,
  stream: &mut Option<tokio::task::JoinHandle<()>>,
  updates: &mpsc::Sender<Update>,
) -> anyhow::Result<()> {
  match request {
    Request::Attach { session, path } => {
      let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
      let image =
        tokio::task::spawn_blocking(move || agent_media::read_explicit_image(&path)).await??;
      let _ = updates
        .send(Update::Media(Reply::Attached {
          session,
          name,
          image,
        }))
        .await;
    }
    Request::Submit {
      session,
      generation,
      submission,
    } => {
      let epoch = client
        .runtime_status(&session)
        .await?
        .session
        .ok_or_else(|| anyhow::anyhow!("session unavailable"))?
        .owner_epoch;
      let result = client
        .start_run_with_attachments_owned(
          &session,
          epoch,
          &submission.prompt,
          &submission.request_id,
          &submission.images,
        )
        .await;
      let run = match result {
        Ok(run) => run,
        Err(error) => {
          if agent_client::media::admission_was_rejected(&error) {
            let _ = updates
              .send(Update::Media(Reply::Rejected {
                session: session.clone(),
                request_id: submission.request_id.clone(),
              }))
              .await;
          }
          return Err(error);
        }
      };
      let _ = updates
        .send(Update::Media(Reply::Confirmed {
          session: session.clone(),
          request_id: submission.request_id,
        }))
        .await;
      let _ = updates
        .send(Update::Submitted {
          session: session.clone(),
          prompt: submission.prompt,
          generation,
        })
        .await;
      let _ = updates
        .send(Update::Notice(format!("已提交图片任务 {}", run.id)))
        .await;
      refresh_session(client, &session, generation, updates).await?;
      if let Some(task) = stream.take() {
        task.abort();
      }
      *stream = Some(watch(client, &run.id, generation, updates).await?);
    }
  }
  Ok(())
}
