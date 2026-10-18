use crate::{Store, event_tx, verify_epoch};
use anyhow::{Result, ensure};
use diesel::{connection::SimpleConnection, prelude::*};
use serde_json::{Value, json};
diesel::table! {
  browser_actions (action_id) {
    action_id -> Text, session_id -> Text, owner_epoch -> BigInt,
    kind -> Text, arguments -> Text, status -> Text, result -> Text,
  }
}
pub(crate) fn initialize(connection: &mut SqliteConnection) -> Result<()> {
  connection.batch_execute(include_str!("browser_actions.sql"))?;
  Ok(())
}
impl Store {
  /// Record before dispatch. Repeated identifiers are rejected even after restart;
  /// a transport failure never authorizes replaying a browser side effect.
  pub fn begin_browser_action(
    &self,
    session: &str,
    epoch: u64,
    action: &str,
    kind: &str,
    arguments: Value,
  ) -> Result<()> {
    self.lock()?.immediate_transaction(|connection| {
      begin_browser_action_tx(connection, session, epoch, action, kind, arguments)
    })
  }
  pub fn finish_browser_action(
    &self,
    action: &str,
    epoch: u64,
    status: &str,
    summary: Value,
  ) -> Result<()> {
    ensure!(
      matches!(status, "completed" | "failed" | "uncertain"),
      "invalid browser outcome"
    );
    let result = serde_json::to_string(&summary)?;
    ensure!(
      result.len() <= 1024 * 1024,
      "browser result summary exceeds limit"
    );
    self.lock()?.immediate_transaction(|connection| {
      let (session, actual): (String, i64) = browser_actions::table
        .find(action)
        .select((browser_actions::session_id, browser_actions::owner_epoch))
        .first(connection)?;
      ensure!(
        u64::try_from(actual)? == epoch,
        "stale browser action epoch"
      );
      verify_epoch(connection, &session, epoch)?;
      ensure!(
        diesel::update(
          browser_actions::table
            .find(action)
            .filter(browser_actions::status.eq("intent"))
        )
        .set((
          browser_actions::status.eq(status),
          browser_actions::result.eq(result)
        ))
        .execute(connection)?
          == 1,
        "browser outcome already recorded"
      );
      event_tx(
        connection,
        &format!("browser:{session}"),
        "browser_action_finished",
        json!({"action_id":action,"status":status}),
      )?;
      Ok(())
    })
  }
  pub fn browser_action_status(&self, action: &str) -> Result<Option<String>> {
    Ok(
      browser_actions::table
        .find(action)
        .select(browser_actions::status)
        .first(&mut *self.lock()?)
        .optional()?,
    )
  }
  pub fn recover_browser_actions(&self) -> Result<usize> {
    self.lock()?.immediate_transaction(|connection| {
      let interrupted = browser_actions::table
        .filter(browser_actions::status.eq("intent"))
        .select((browser_actions::action_id, browser_actions::session_id))
        .load::<(String, String)>(connection)?;
      for (id, session) in &interrupted {
        diesel::update(browser_actions::table.find(id))
          .set(browser_actions::status.eq("uncertain"))
          .execute(connection)?;
        event_tx(
          connection,
          &format!("browser:{session}"),
          "browser_action_finished",
          json!({"action_id":id,"status":"uncertain","reason":"daemon_restarted"}),
        )?;
      }
      Ok(interrupted.len())
    })
  }
}

/// Shared by authenticated human intent and atomic model approval/intent writes.
pub(crate) fn begin_browser_action_tx(
  connection: &mut SqliteConnection,
  session: &str,
  epoch: u64,
  action: &str,
  kind: &str,
  arguments: Value,
) -> Result<()> {
  ensure!(
    !action.is_empty() && action.len() <= 128,
    "invalid browser action identifier"
  );
  let arguments = serde_json::to_string(&arguments)?;
  ensure!(
    arguments.len() <= 1024 * 1024,
    "browser action arguments exceed limit"
  );
  verify_epoch(connection, session, epoch)?;
  let prior = browser_actions::table
    .find(action)
    .select(browser_actions::action_id)
    .first::<String>(connection)
    .optional()?;
  ensure!(
    prior.is_none(),
    "browser action already recorded; inspect its outcome before using a new action identifier"
  );
  diesel::insert_into(browser_actions::table)
    .values((
      browser_actions::action_id.eq(action),
      browser_actions::session_id.eq(session),
      browser_actions::owner_epoch.eq(i64::try_from(epoch)?),
      browser_actions::kind.eq(kind),
      browser_actions::arguments.eq(arguments),
      browser_actions::status.eq("intent"),
      browser_actions::result.eq(""),
    ))
    .execute(connection)?;
  event_tx(
    connection,
    &format!("browser:{session}"),
    "browser_action_intent",
    json!({"action_id":action,"kind":kind}),
  )?;
  Ok(())
}
