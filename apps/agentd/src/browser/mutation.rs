//! Durable browser dispatch shared by human commands and approved model tools.
use super::routing::failure;
use crate::Daemon;
use agent_browser::{BrowserError, Command, Reply};
use agent_storage::{BrowserActionAuthorization, Store};
use serde_json::json;
use tonic::Status;

pub(super) struct CommittedMutation {
  session: String,
  host: String,
  command: Command,
  guard: MutationGuard,
}

impl CommittedMutation {
  /// Consume a post-commit proof and capture the command before any await.
  /// Only the model adapter constructs this, from its already-approved operation.
  pub(super) fn approved(
    store: Store,
    proof: BrowserActionAuthorization,
    command: Command,
  ) -> Self {
    Self {
      session: proof.session_id().into(),
      host: proof.host_id().into(),
      command,
      guard: MutationGuard {
        store,
        id: proof.action_id().into(),
        epoch: proof.owner_epoch(),
        finished: false,
      },
    }
  }

  pub(super) async fn execute(mut self, daemon: &Daemon) -> Result<Reply, Status> {
    let expected = self.command.clone();
    let result = daemon
      .browser
      .request(&self.host, self.command)
      .await
      .and_then(|reply| {
        if super::validation::matches_reply(&expected, &reply, &self.session) {
          Ok(reply)
        } else {
          Err(BrowserError::Uncertain)
        }
      });
    let status = match &result {
      Ok(_) => "completed",
      Err(BrowserError::Uncertain | BrowserError::Timeout) => "uncertain",
      Err(_) => "failed",
    };
    self
      .guard
      .store
      .finish_browser_action(
        &self.guard.id,
        self.guard.epoch,
        status,
        json!({"status":status}),
      )
      .map_err(|_| Status::unknown("browser outcome could not be persisted; do not replay"))?;
    self.guard.finished = true;
    result.map_err(failure)
  }
}

impl Daemon {
  /// Authenticated human commands have their own intent path and share receipts.
  pub(super) async fn browser_mutate(
    &self,
    session_id: &str,
    host: &str,
    id: &str,
    command: Command,
  ) -> Result<Reply, Status> {
    let session = self
      .store
      .session(session_id)
      .map_err(|_| Status::internal("session storage failed"))?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::failed_precondition(
        "this node does not own the browser session",
      ));
    }
    self
      .store
      .begin_browser_action(
        session_id,
        session.owner_epoch,
        id,
        "browser_command",
        audit_summary(&command),
      )
      .map_err(|_| {
        Status::failed_precondition("browser action already recorded or session ownership changed")
      })?;
    let mutation = CommittedMutation {
      session: session_id.into(),
      host: host.into(),
      command,
      guard: MutationGuard {
        store: self.store.clone(),
        id: id.into(),
        epoch: session.owner_epoch,
        finished: false,
      },
    };
    mutation.execute(self).await
  }
}

/// Human keystrokes, fill values and navigation URLs can contain credentials.
/// Receipts need identity and outcome, never a reconstructable input stream.
pub(super) fn audit_summary(command: &Command) -> serde_json::Value {
  match command {
    Command::Input {
      tab_id, revision, ..
    } => json!({"kind":"input","tab_id":tab_id,"revision":revision}),
    Command::Act(action) => {
      json!({"kind":"act","action_id":action.id,"tab_id":action.tab_id,"snapshot_id":action.snapshot_id,"revision":action.revision,"operation":action.kind})
    }
    Command::OpenTab { space_id, .. } => json!({"kind":"open_tab","space_id":space_id}),
    Command::OpenSpace {
      session_id,
      profile_id,
      headless,
    } => {
      json!({"kind":"open_space","session_id":session_id,"profile_id":profile_id,"headless":headless})
    }
    Command::Takeover {
      space_id,
      human_control,
    } => json!({"kind":"takeover","space_id":space_id,"human_control":human_control}),
    Command::CloseSpace { space_id } => json!({"kind":"close_space","space_id":space_id}),
    _ => json!({"kind":"runtime_command"}),
  }
}

struct MutationGuard {
  store: agent_storage::Store,
  id: String,
  epoch: u64,
  finished: bool,
}
impl Drop for MutationGuard {
  fn drop(&mut self) {
    if !self.finished
      && self
        .store
        .finish_browser_action(
          &self.id,
          self.epoch,
          "uncertain",
          json!({"reason":"caller_disconnected"}),
        )
        .is_err()
    {
      tracing::warn!(action_id=%self.id,"browser intent remains unresolved after caller disconnected");
    }
  }
}
