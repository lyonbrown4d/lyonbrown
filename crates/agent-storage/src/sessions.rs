use crate::{Store, decode, event_tx, schema::sessions, verify_epoch};
use agent_core::{Session, new_id, now_ms};
use anyhow::{Context, Result};
use diesel::prelude::*;
impl Store {
  pub fn create_session(&self, title: &str, workspace: &str, node: &str) -> Result<Session> {
    anyhow::ensure!(title.len() <= 1024, "session title too long");
    let value = Session {
      id: new_id(),
      title: title.into(),
      workspace: workspace.into(),
      owner_node_id: node.into(),
      owner_epoch: 1,
      created_at_ms: now_ms(),
    };
    diesel::insert_into(sessions::table)
      .values((
        sessions::id.eq(&value.id),
        sessions::owner_epoch.eq(1_i64),
        sessions::record.eq(serde_json::to_string(&value)?),
      ))
      .execute(&mut *self.lock()?)?;
    Ok(value)
  }
  pub fn session(&self, id: &str) -> Result<Option<Session>> {
    let json: Option<String> = sessions::table
      .find(id)
      .select(sessions::record)
      .first(&mut *self.lock()?)
      .optional()?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
  pub fn sessions(&self) -> Result<Vec<Session>> {
    decode(
      sessions::table
        .order(sessions::id.desc())
        .select(sessions::record)
        .load(&mut *self.lock()?)?,
    )
  }
  pub fn change_owner(&self, id: &str, expected_epoch: u64, node: &str) -> Result<Session> {
    self.lock()?.immediate_transaction(|connection| {
      verify_epoch(connection, id, expected_epoch)?;
      let json: String = sessions::table
        .find(id)
        .select(sessions::record)
        .first(connection)?;
      let mut value: Session = serde_json::from_str(&json)?;
      value.owner_epoch = expected_epoch
        .checked_add(1)
        .context("owner epoch exhausted")?;
      value.owner_node_id = node.into();
      diesel::update(sessions::table.find(id))
        .set((
          sessions::owner_epoch.eq(i64::try_from(value.owner_epoch)?),
          sessions::record.eq(serde_json::to_string(&value)?),
        ))
        .execute(connection)?;
      event_tx(
        connection,
        id,
        "session_owner_changed",
        serde_json::json!({"owner_node_id":node,"owner_epoch":value.owner_epoch}),
      )?;
      Ok(value)
    })
  }
}
