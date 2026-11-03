use crate::{Store, event_tx, schema::runs, verify_epoch};
use agent_core::{Run, new_id, now_ms};
use anyhow::Result;
use sea_orm::{QuerySelect, Set, prelude::*};
impl Store {
  pub async fn accept_run_with_media(
    &self,
    session: &str,
    epoch: u64,
    prompt: &str,
    key: &str,
    images: &[agent_media::ImageAttachment],
  ) -> Result<(Run, bool)> {
    anyhow::ensure!(
      !key.is_empty() && key.len() <= 256,
      "invalid idempotency key"
    );
    agent_media::validate_images(prompt, images)?;
    if !images.is_empty() {
      uuid::Uuid::parse_str(key)?;
    }
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        verify_epoch(connection, session, epoch).await?;
        let existing: Option<String> = runs::Entity::find()
          .filter(runs::Column::SessionId.eq(session))
          .filter(runs::Column::IdempotencyKey.eq(key))
          .select_only()
          .column(runs::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?;
        if let Some(json) = existing {
          let run: Run = serde_json::from_str(&json)?;
          anyhow::ensure!(
            run.prompt == prompt,
            "idempotency key reused with different prompt"
          );
          let existing_images = super::load(connection, &run.id).await?;
          anyhow::ensure!(
            existing_images.as_slice() == images,
            "idempotency key reused with different attachments"
          );
          return Ok((run, false));
        }
        crate::context_maintenance::ensure_idle(connection, session).await?;
        crate::input_fence::ensure_no_input_cancellation(connection, session).await?;
        crate::changes::ensure_no_prepared_change(connection, session).await?;
        let active: i64 = runs::Entity::find()
          .filter(runs::Column::SessionId.eq(session))
          .filter(runs::Column::Status.is_in([
            "accepted",
            "running",
            "awaiting_approval",
            "cancel_requested",
            "uncertain",
          ]))
          .count(connection)
          .await
          .map(|value| value as i64)?;
        anyhow::ensure!(
          active == 0,
          "session already has an active run; use explicit steering or follow-up input"
        );
        let run = Run {
          id: new_id(),
          session_id: session.into(),
          status: "accepted".into(),
          prompt: prompt.into(),
          result: String::new(),
          error: String::new(),
          created_at_ms: now_ms(),
        };
        use crate::schema::model_bindings;
        if let Some(record) = model_bindings::Entity::find()
          .filter(model_bindings::Column::ScopeId.eq(session))
          .select_only()
          .column(model_bindings::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?
        {
          model_bindings::ActiveModel {
            scope_id: Set((&run.id).to_owned()),
            record: Set((record).to_owned()),
          }
          .insert(connection)
          .await?;
        }
        runs::ActiveModel {
          id: Set((&run.id).to_owned()),
          session_id: Set((session).to_owned()),
          idempotency_key: Set((key).to_owned()),
          status: Set((&run.status).to_owned()),
          created_at_ms: Set((run.created_at_ms).to_owned()),
          record: Set((serde_json::to_string(&run)?).to_owned()),
        }
        .insert(connection)
        .await?;
        for (ordinal, image) in images.iter().enumerate() {
          super::entities::ActiveModel {
            run_id: Set(run.id.clone()),
            ordinal: Set(i32::try_from(ordinal)?),
            mime_type: Set(image.mime_type.clone()),
            sha256: Set(image.sha256.clone()),
            data: Set(image.data.clone()),
          }
          .insert(connection)
          .await?;
        }
        crate::interaction::bind_run_mode(connection, session, &run.id).await?;
        event_tx(
          connection,
          &run.id,
          "run_accepted",
          serde_json::json!({"run_id":run.id,"image_count":images.len()}),
        )
        .await?;
        Ok((run, true))
      }
      .await;
      match result {
        Ok(value) => {
          transaction.commit().await?;
          Ok::<_, anyhow::Error>(value)
        }
        Err(error) => {
          transaction.rollback().await?;
          Err(error)
        }
      }
    }
  }
}
