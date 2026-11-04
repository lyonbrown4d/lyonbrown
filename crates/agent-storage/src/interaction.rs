use crate::{Store, event_tx, verify_epoch};
use agent_core::interaction::{InteractionMode, InteractionPolicy};
use anyhow::{Result, ensure};
use sea_orm::{DatabaseTransaction, QuerySelect, Set, prelude::*, sea_query::OnConflict};
mod rows {
  use sea_orm::entity::prelude::*;
  #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
  #[sea_orm(table_name = "interaction_policies")]
  pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[sea_orm(column_type = "String(StringLen::N(256))")]
    pub scope_id: String,
    #[sea_orm(column_type = "Text")]
    pub record: String,
  }
  #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
  pub enum Relation {}
  impl ActiveModelBehavior for ActiveModel {}
}
async fn read<C: ConnectionTrait>(db: &C, scope: &str) -> Result<InteractionPolicy> {
  let record = rows::Entity::find_by_id(scope)
    .select_only()
    .column(rows::Column::Record)
    .into_tuple::<String>()
    .one(db)
    .await?;
  record
    .map(|s| serde_json::from_str(&s).map_err(Into::into))
    .transpose()
    .map(|value| value.unwrap_or_default())
}
async fn write(db: &DatabaseTransaction, scope: &str, policy: &InteractionPolicy) -> Result<()> {
  rows::Entity::insert(rows::ActiveModel {
    scope_id: Set(scope.into()),
    record: Set(serde_json::to_string(policy)?),
  })
  .on_conflict(
    OnConflict::column(rows::Column::ScopeId)
      .update_column(rows::Column::Record)
      .to_owned(),
  )
  .exec_without_returning(db)
  .await?;
  Ok(())
}
pub(crate) async fn bind_run_mode(
  db: &DatabaseTransaction,
  session: &str,
  run: &str,
) -> Result<()> {
  write(db, run, &read(db, session).await?).await
}
pub(crate) async fn inherit_recovery_mode(
  db: &DatabaseTransaction,
  source_run: &str,
  session: &str,
  run: &str,
) -> Result<()> {
  let policy = read_required(db, source_run).await?;
  write(db, session, &policy).await?;
  write(db, run, &policy).await
}
async fn read_required<C: ConnectionTrait>(db: &C, scope: &str) -> Result<InteractionPolicy> {
  let row = rows::Entity::find_by_id(scope)
    .one(db)
    .await?
    .ok_or_else(|| anyhow::anyhow!("run interaction binding missing"))?;
  Ok(serde_json::from_str(&row.record)?)
}
impl Store {
  pub async fn run_interaction_policy(&self, run_id: &str) -> Result<InteractionPolicy> {
    read_required(self.connection(), run_id).await
  }
  pub async fn interaction_policy(&self, scope: &str) -> Result<InteractionPolicy> {
    read(self.connection(), scope).await
  }
  pub async fn set_interaction_mode(
    &self,
    session: &str,
    epoch: u64,
    expected_revision: u64,
    mode: InteractionMode,
  ) -> Result<InteractionPolicy> {
    let transaction = self.begin_write().await?;
    let result = async {
      verify_epoch(&transaction, session, epoch).await?;
      crate::context_maintenance::ensure_idle(&transaction, session).await?;
      let active = crate::schema::runs::Entity::find()
        .filter(crate::schema::runs::Column::SessionId.eq(session))
        .filter(crate::schema::runs::Column::Status.is_in([
          "accepted",
          "running",
          "awaiting_approval",
          "cancel_requested",
          "uncertain",
        ]))
        .count(&transaction)
        .await?;
      ensure!(
        active == 0,
        "reconcile or finish the current run before changing mode"
      );
      let current = read(&transaction, session).await?;
      ensure!(
        current.revision == expected_revision,
        "interaction mode revision changed"
      );
      let policy = InteractionPolicy {
        revision: current
          .revision
          .checked_add(1)
          .ok_or_else(|| anyhow::anyhow!("mode revision exhausted"))?,
        mode,
      };
      write(&transaction, session, &policy).await?;
      event_tx(
        &transaction,
        session,
        "interaction_mode_changed",
        serde_json::to_value(&policy)?,
      )
      .await?;
      Ok(policy)
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
