use crate::{
  Store, event_tx,
  schema::{runs as run_rows, runtime_sessions},
  verify_epoch,
};
use anyhow::{Context, Result};
use sea_orm::{QueryOrder, Set, prelude::*, sea_query::OnConflict};
impl Store {
  pub async fn load_snapshot(&self, session_id: &str) -> Result<Option<Vec<u8>>> {
    Ok(
      runtime_sessions::Entity::find_by_id(session_id)
        .one(self.connection())
        .await?
        .map(|row| row.snapshot),
    )
  }
  pub async fn commit_snapshot(
    &self,
    session_id: &str,
    epoch: u64,
    snapshot: &[u8],
    kind: &str,
    payload: serde_json::Value,
  ) -> Result<()> {
    self
      .commit_snapshot_with_events(session_id, epoch, snapshot, kind, payload, &[])
      .await
  }
  pub async fn commit_snapshot_with_events(
    &self,
    session_id: &str,
    epoch: u64,
    snapshot: &[u8],
    kind: &str,
    payload: serde_json::Value,
    semantic_events: &[agent_core::events::AgentEvent],
  ) -> Result<()> {
    self
      .commit_run_snapshot_with_events(
        session_id,
        epoch,
        None,
        snapshot,
        kind,
        payload,
        semantic_events,
      )
      .await
  }
  #[allow(clippy::too_many_arguments)]
  pub async fn commit_run_snapshot_with_events(
    &self,
    session_id: &str,
    epoch: u64,
    run_id: Option<&str>,
    snapshot: &[u8],
    kind: &str,
    payload: serde_json::Value,
    semantic_events: &[agent_core::events::AgentEvent],
  ) -> Result<()> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        verify_epoch(connection, session_id, epoch).await?;
        let run = if let Some(id) = run_id {
          let run = run_rows::Entity::find_by_id(id)
            .one(connection)
            .await?
            .context("run missing")?;
          anyhow::ensure!(
            run.session_id == session_id,
            "runtime run belongs to another session"
          );
          run
        } else {
          run_rows::Entity::find()
            .filter(run_rows::Column::SessionId.eq(session_id))
            .order_by_desc(run_rows::Column::CreatedAtMs)
            .order_by_desc(run_rows::Column::Id)
            .one(connection)
            .await?
            .context("session has no accepted run for runtime mutation")?
        };
        crate::context_maintenance::observe_snapshot(connection, session_id, epoch, kind).await?;
        runtime_sessions::Entity::insert(runtime_sessions::ActiveModel {
          session_id: Set(session_id.into()),
          snapshot: Set(snapshot.into()),
        })
        .on_conflict(
          OnConflict::column(runtime_sessions::Column::SessionId)
            .update_column(runtime_sessions::Column::Snapshot)
            .to_owned(),
        )
        .exec_without_returning(connection)
        .await?;
        event_tx(connection, &run.id, kind, payload).await?;
        for event in semantic_events {
          event_tx(
            connection,
            &run.id,
            agent_core::events::AGENT_EVENT_KIND,
            serde_json::to_value(event)?,
          )
          .await?;
        }
        Ok(())
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
