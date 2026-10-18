use crate::{Store, decode, event_tx, schema::runs, verify_epoch};
use agent_core::{Run, RunStatus, new_id, now_ms};
use anyhow::Result;
use diesel::prelude::*;
impl Store {
  pub fn accept_run(
    &self,
    session: &str,
    epoch: u64,
    prompt: &str,
    key: &str,
  ) -> Result<(Run, bool)> {
    anyhow::ensure!(
      !key.is_empty() && key.len() <= 256,
      "invalid idempotency key"
    );
    anyhow::ensure!(
      !prompt.trim().is_empty() && prompt.len() <= 1_048_576,
      "invalid prompt length"
    );
    self.lock()?.immediate_transaction(|connection| {
      verify_epoch(connection, session, epoch)?;
      let existing: Option<String> = runs::table
        .filter(runs::session_id.eq(session))
        .filter(runs::idempotency_key.eq(key))
        .select(runs::record)
        .first(connection)
        .optional()?;
      if let Some(json) = existing {
        let run: Run = serde_json::from_str(&json)?;
        anyhow::ensure!(
          run.prompt == prompt,
          "idempotency key reused with different prompt"
        );
        return Ok((run, false));
      }
      let run = Run {
        id: new_id(),
        session_id: session.into(),
        status: "accepted".into(),
        prompt: prompt.into(),
        result: String::new(),
        error: String::new(),
        created_at_ms: now_ms(),
      };
      diesel::insert_into(runs::table)
        .values((
          runs::id.eq(&run.id),
          runs::session_id.eq(session),
          runs::idempotency_key.eq(key),
          runs::status.eq(&run.status),
          runs::created_at_ms.eq(run.created_at_ms),
          runs::record.eq(serde_json::to_string(&run)?),
        ))
        .execute(connection)?;
      event_tx(
        connection,
        &run.id,
        "run_accepted",
        serde_json::json!({"run_id":run.id}),
      )?;
      Ok((run, true))
    })
  }
  pub fn run(&self, id: &str) -> Result<Option<Run>> {
    let json: Option<String> = runs::table
      .find(id)
      .select(runs::record)
      .first(&mut *self.lock()?)
      .optional()?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
  pub fn runs(&self, session: &str) -> Result<Vec<Run>> {
    decode(
      runs::table
        .filter(runs::session_id.eq(session))
        .order((runs::created_at_ms.asc(), runs::id.asc()))
        .select(runs::record)
        .load(&mut *self.lock()?)?,
    )
  }
  pub fn update_run(&self, id: &str, status: RunStatus, result: &str, error: &str) -> Result<Run> {
    self.mutate_run(id, None, status, result, error)
  }
  pub fn update_run_owned(
    &self,
    id: &str,
    epoch: u64,
    status: RunStatus,
    result: &str,
    error: &str,
  ) -> Result<Run> {
    self.mutate_run(id, Some(epoch), status, result, error)
  }
  fn mutate_run(
    &self,
    id: &str,
    epoch: Option<u64>,
    status: RunStatus,
    result: &str,
    error: &str,
  ) -> Result<Run> {
    self.lock()?.immediate_transaction(|connection| {
      let json: String = runs::table
        .find(id)
        .select(runs::record)
        .first(connection)?;
      let mut run: Run = serde_json::from_str(&json)?;
      if let Some(epoch) = epoch {
        verify_epoch(connection, &run.session_id, epoch)?;
      }
      anyhow::ensure!(
        !matches!(
          run.status.as_str(),
          "completed" | "failed" | "cancelled" | "uncertain"
        ),
        "run is already terminal"
      );
      // Serialize cancellation against the terminal commit. A successful
      // Harness completion cannot overwrite a cancellation already accepted
      // by the daemon. Unknown side effects retain their Uncertain outcome.
      let status = if run.status == "cancel_requested" && status == RunStatus::Completed {
        RunStatus::Cancelled
      } else {
        status
      };
      run.status = status.as_str().into();
      run.result = result.into();
      run.error = error.into();
      if status.terminal() {
        crate::approvals::close_pending(connection, id)?;
      }
      diesel::update(runs::table.find(id))
        .set((
          runs::status.eq(&run.status),
          runs::record.eq(serde_json::to_string(&run)?),
        ))
        .execute(connection)?;
      event_tx(
        connection,
        id,
        &format!("run_{}", status.as_str()),
        serde_json::json!({"status":run.status,"result":result,"error":error}),
      )?;
      Ok(run)
    })
  }
  pub fn recover_interrupted(&self) -> Result<Vec<Run>> {
    self.lock()?.immediate_transaction(|connection| {
      let records: Vec<String> = runs::table
        .filter(runs::status.ne_all(["accepted", "completed", "failed", "cancelled", "uncertain"]))
        .select(runs::record)
        .load(connection)?;
      let mut values = Vec::new();
      for record in records {
        let mut run: Run = serde_json::from_str(&record)?;
        run.status = "uncertain".into();
        run.error =
          "daemon stopped before a durable terminal result; reconcile before resuming".into();
        crate::approvals::close_pending(connection, &run.id)?;
        diesel::update(runs::table.find(&run.id))
          .set((
            runs::status.eq(&run.status),
            runs::record.eq(serde_json::to_string(&run)?),
          ))
          .execute(connection)?;
        event_tx(
          connection,
          &run.id,
          "run_uncertain",
          serde_json::json!({"error":run.error}),
        )?;
        values.push(run);
      }
      Ok(values)
    })
  }
  pub fn pending_runs(&self) -> Result<Vec<Run>> {
    decode(
      runs::table
        .filter(runs::status.eq("accepted"))
        .order((runs::created_at_ms.asc(), runs::id.asc()))
        .select(runs::record)
        .load(&mut *self.lock()?)?,
    )
  }
}
