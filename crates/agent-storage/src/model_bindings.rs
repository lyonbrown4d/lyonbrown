use crate::{
  Store, event_tx,
  schema::{model_bindings, runs},
  verify_epoch,
};
use diesel::prelude::*;
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
  pub fn recover_model_jobs(&self) -> anyhow::Result<usize> {
    use crate::schema::model_jobs;
    let records = model_jobs::table
      .select((model_jobs::id, model_jobs::record))
      .load::<(String, String)>(&mut *self.lock()?)?;
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
        self.record_model_job(&id, value)?;
        recovered += 1;
      }
    }
    Ok(recovered)
  }
  pub fn model_job(&self, id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    use crate::schema::model_jobs;
    let record = model_jobs::table
      .find(id)
      .select(model_jobs::record)
      .first::<String>(&mut *self.lock()?)
      .optional()?;
    record
      .map(|value| serde_json::from_str(&value).map_err(Into::into))
      .transpose()
  }
  pub fn record_model_job(&self, id: &str, state: serde_json::Value) -> anyhow::Result<()> {
    use crate::schema::model_jobs;
    self.lock()?.immediate_transaction(|connection| {
      let record = serde_json::to_string(&state)?;
      diesel::insert_into(model_jobs::table)
        .values((model_jobs::id.eq(id), model_jobs::record.eq(&record)))
        .on_conflict(model_jobs::id)
        .do_update()
        .set(model_jobs::record.eq(&record))
        .execute(connection)?;
      event_tx(connection, id, "model_job_changed", state)?;
      Ok(())
    })
  }
  pub fn model_binding(&self, scope: &str) -> anyhow::Result<Option<ModelBinding>> {
    let record: Option<String> = model_bindings::table
      .find(scope)
      .select(model_bindings::record)
      .first(&mut *self.lock()?)
      .optional()?;
    record
      .map(|text| serde_json::from_str(&text).map_err(Into::into))
      .transpose()
  }
  pub fn select_model(
    &self,
    session: &str,
    epoch: u64,
    binding: &ModelBinding,
  ) -> anyhow::Result<()> {
    self.lock()?.immediate_transaction(|connection| {
      verify_epoch(connection, session, epoch)?;
      let active = runs::table
        .filter(runs::session_id.eq(session))
        .filter(runs::status.ne_all(["completed", "failed", "cancelled"]))
        .count()
        .get_result::<i64>(connection)?;
      anyhow::ensure!(
        active == 0,
        "cannot change model while a run is active or uncertain"
      );
      diesel::insert_into(model_bindings::table)
        .values((
          model_bindings::scope_id.eq(session),
          model_bindings::record.eq(serde_json::to_string(binding)?),
        ))
        .on_conflict(model_bindings::scope_id)
        .do_update()
        .set(model_bindings::record.eq(serde_json::to_string(binding)?))
        .execute(connection)?;
      event_tx(
        connection,
        session,
        "model_selected",
        serde_json::to_value(binding)?,
      )?;
      Ok(())
    })
  }
}
