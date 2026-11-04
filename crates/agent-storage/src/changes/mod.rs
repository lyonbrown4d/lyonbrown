//! Durable reviewed changes; filesystem effects are deliberately outside the transaction.
mod entities;
mod fence;
pub(crate) use fence::ensure_no_prepared_change;
mod mutation;
mod read;
mod recovery;
mod user;
mod validation;
use crate::{Store, event_tx, schema::runs, verify_epoch};
use agent_changes::{ChangeSet, ChangeState};
use anyhow::{Context, Result, ensure};
use entities::{requests, sets};
pub use mutation::{PrepareChange, PreparedChange};
use sea_orm::{QuerySelect, Set, prelude::*};
pub use user::UserPrepareChange;

pub(super) async fn load(connection: &impl ConnectionTrait, id: &str) -> Result<ChangeSet> {
  let row = sets::Entity::find_by_id(id)
    .one(connection)
    .await?
    .context("change set missing")?;
  Ok(serde_json::from_str(&row.record)?)
}
pub(super) async fn fence(connection: &impl ConnectionTrait, run: &str, epoch: u64) -> Result<()> {
  let (session, status): (String, String) = runs::Entity::find_by_id(run)
    .select_only()
    .column(runs::Column::SessionId)
    .column(runs::Column::Status)
    .into_tuple()
    .one(connection)
    .await?
    .context("change run missing")?;
  verify_epoch(connection, &session, epoch).await?;
  ensure!(
    status == "running",
    "change requires a running uncancelled run"
  );
  Ok(())
}
pub(super) async fn update(
  connection: &impl ConnectionTrait,
  change: &ChangeSet,
  expected: u64,
) -> Result<()> {
  ensure!(
    change.revision
      == expected
        .checked_add(1)
        .context("change revision overflow")?,
    "invalid change revision"
  );
  let changed = sets::Entity::update_many()
    .filter(sets::Column::Id.eq(&change.id))
    .filter(sets::Column::Revision.eq(i64::try_from(expected)?))
    .col_expr(
      sets::Column::Revision,
      Expr::value(i64::try_from(change.revision)?),
    )
    .col_expr(sets::Column::State, Expr::value(change.state.as_str()))
    .col_expr(
      sets::Column::Record,
      Expr::value(serde_json::to_string(change)?),
    )
    .exec(connection)
    .await?;
  ensure!(changed.rows_affected == 1, "change revision conflict");
  event_tx(connection, &change.run_id, "change_updated", serde_json::json!({"change_id":change.id,"revision":change.revision,"state":change.state,"preview_sha256":change.preview_sha256})).await?;
  Ok(())
}
pub(super) async fn request(
  connection: &impl ConnectionTrait,
  id: &str,
  body: &str,
) -> Result<Option<String>> {
  uuid::Uuid::parse_str(id)?;
  let prior = requests::Entity::find_by_id(id).one(connection).await?;
  prior
    .map(|row| {
      ensure!(
        row.body_hash == body,
        "request UUID reused with different arguments"
      );
      Ok(row.record)
    })
    .transpose()
}
pub(super) async fn record_request(
  connection: &impl ConnectionTrait,
  id: &str,
  hash: &str,
  record: String,
) -> Result<()> {
  requests::ActiveModel {
    id: Set(id.into()),
    body_hash: Set(hash.into()),
    record: Set(record),
  }
  .insert(connection)
  .await?;
  Ok(())
}
impl Store {
  pub async fn change_set(&self, id: &str) -> Result<ChangeSet> {
    load(self.connection(), id).await
  }

  pub async fn save_change_preview(
    &self,
    request_id: &str,
    change: ChangeSet,
  ) -> Result<ChangeSet> {
    agent_changes::validate(&change)?;
    ensure!(
      change.state == ChangeState::Preview
        && change.revision == 1
        && change.action_id.is_none()
        && change.written_paths.is_empty()
        && change.applied_event_cursor.is_none(),
      "invalid change preview state"
    );
    let body = agent_changes::digest(&serde_json::to_vec(&change)?);
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      fence(&transaction, &change.run_id, change.owner_epoch).await?;
      if let Some(record) = request(&transaction, request_id, &body).await? { return Ok(serde_json::from_str(&record)?); }
      sets::ActiveModel {
        id: Set(change.id.clone()), run_id: Set(change.run_id.clone()), revision: Set(1),
        state: Set("preview".into()), record: Set(serde_json::to_string(&change)?),
      }.insert(&transaction).await?;
      record_request(&transaction, request_id, &body, serde_json::to_string(&change)?).await?;
      event_tx(&transaction, &change.run_id, "change_previewed", serde_json::json!({"change_id":change.id,"preview_sha256":change.preview_sha256,"files":change.files.len()})).await?;
      Ok(change)
    }.await;
    match result {
      Ok(value) => {
        transaction.commit().await?;
        Ok(value)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }

  /// An interrupted prepared change is inspectable, never replayed automatically.
  pub async fn recover_changes(&self) -> Result<usize> {
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let rows = sets::Entity::find()
        .filter(sets::Column::State.eq("prepared"))
        .all(&transaction)
        .await?;
      for row in &rows {
        let mut change: ChangeSet = serde_json::from_str(&row.record)?;
        let expected = change.revision;
        change.revision = expected
          .checked_add(1)
          .context("change revision overflow")?;
        change.state = ChangeState::Uncertain;
        recovery::recover_intent(
          &transaction,
          change
            .action_id
            .as_deref()
            .context("prepared change intent missing")?,
        )
        .await?;
        update(&transaction, &change, expected).await?;
      }
      Ok(rows.len())
    }
    .await;
    match result {
      Ok(value) => {
        transaction.commit().await?;
        Ok(value)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
}
