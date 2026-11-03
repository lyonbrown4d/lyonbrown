use crate::{
  Store, event_tx,
  schema::{model_bindings, runs},
  verify_epoch,
};
use sea_orm::{QuerySelect, Set, prelude::*, sea_query::OnConflict};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelBinding {
  pub model_id: String,
  pub execution_id: String,
  pub provider: String,
  pub model: String,
  pub endpoint: Option<String>,
  pub credential_id: String,
}
impl Store {
  pub async fn recover_model_jobs(&self) -> anyhow::Result<usize> {
    use crate::schema::model_jobs;
    let records = model_jobs::Entity::find()
      .select_only()
      .column(model_jobs::Column::Id)
      .column(model_jobs::Column::Record)
      .into_tuple::<(String, String)>()
      .all(self.connection())
      .await?;
    let mut recovered = 0;
    for (id, record) in records {
      let mut value: serde_json::Value = serde_json::from_str(&record)?;
      if matches!(
        value["state"].as_str(),
        Some("accepted" | "downloading" | "activating")
      ) {
        value["state"] = "uncertain".into();
        value["reason"] =
          "daemon restarted; inspect artifacts and explicitly resume installation".into();
        self.record_model_job(&id, value).await?;
        recovered += 1;
      }
    }
    Ok(recovered)
  }
  pub async fn model_job(&self, id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    use crate::schema::model_jobs;
    let record = model_jobs::Entity::find()
      .filter(model_jobs::Column::Id.eq(id))
      .select_only()
      .column(model_jobs::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    record
      .map(|value| serde_json::from_str(&value).map_err(Into::into))
      .transpose()
  }
  pub async fn record_model_job(&self, id: &str, state: serde_json::Value) -> anyhow::Result<()> {
    use crate::schema::model_jobs;
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let record = serde_json::to_string(&state)?;
        model_jobs::Entity::insert(model_jobs::ActiveModel {
          id: Set((id).to_owned()),
          record: Set(record.to_owned()),
        })
        .on_conflict(
          OnConflict::columns([model_jobs::Column::Id])
            .update_columns([model_jobs::Column::Record])
            .to_owned(),
        )
        .exec_without_returning(connection)
        .await?;
        event_tx(connection, id, "model_job_changed", state).await?;
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
  pub async fn model_binding(&self, scope: &str) -> anyhow::Result<Option<ModelBinding>> {
    let record: Option<String> = model_bindings::Entity::find()
      .filter(model_bindings::Column::ScopeId.eq(scope))
      .select_only()
      .column(model_bindings::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    record
      .map(|text| serde_json::from_str(&text).map_err(Into::into))
      .transpose()
  }
  pub async fn select_model(
    &self,
    session: &str,
    epoch: u64,
    binding: &ModelBinding,
  ) -> anyhow::Result<()> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        verify_epoch(connection, session, epoch).await?;
        let active = runs::Entity::find()
          .filter(runs::Column::SessionId.eq(session))
          .filter(runs::Column::Status.is_not_in(["completed", "failed", "cancelled"]))
          .count(connection)
          .await
          .map(|value| value as i64)?;
        anyhow::ensure!(
          active == 0,
          "cannot change model while a run is active or uncertain"
        );
        model_bindings::Entity::insert(model_bindings::ActiveModel {
          scope_id: Set((session).to_owned()),
          record: Set((serde_json::to_string(binding)?).to_owned()),
        })
        .on_conflict(
          OnConflict::columns([model_bindings::Column::ScopeId])
            .update_columns([model_bindings::Column::Record])
            .to_owned(),
        )
        .exec_without_returning(connection)
        .await?;
        event_tx(
          connection,
          session,
          "model_selected",
          serde_json::to_value(binding)?,
        )
        .await?;
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
