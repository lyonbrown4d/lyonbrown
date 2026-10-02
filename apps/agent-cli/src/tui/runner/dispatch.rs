use std::{collections::HashMap, sync::Arc};

use agent_app::{AgentClient, AgentClientError, AgentCommand, AvailabilityKind};
use anyhow::{Context, Result as AnyResult};
use tokio::task::{Id, JoinSet};

/// Keeps command RPCs off the terminal event loop while retaining cancellation ownership.
pub struct CommandDispatcher {
  tasks: JoinSet<Result<(), AgentClientError>>,
  origins: HashMap<Id, CommandOrigin>,
}

pub struct CommandCompletion {
  expected_server_instance_id: String,
  model_setup_token: Option<u64>,
  result: Result<(), AgentClientError>,
  task_failure: bool,
}

struct CommandOrigin {
  expected_server_instance_id: String,
  model_setup_token: Option<u64>,
}

impl CommandDispatcher {
  pub fn new() -> Self {
    Self {
      tasks: JoinSet::new(),
      origins: HashMap::new(),
    }
  }

  pub fn dispatch_for_server(
    &mut self,
    client: Arc<dyn AgentClient>,
    expected_server_instance_id: String,
    command: AgentCommand,
  ) {
    self.dispatch_with_origin(client, expected_server_instance_id, command, None);
  }

  pub fn dispatch_model_setup(
    &mut self,
    client: Arc<dyn AgentClient>,
    expected_server_instance_id: String,
    command: AgentCommand,
    token: u64,
  ) {
    self.dispatch_with_origin(client, expected_server_instance_id, command, Some(token));
  }

  pub fn drain_completions(&mut self) -> Vec<CommandCompletion> {
    let mut completions = Vec::new();
    while let Some(result) = self.tasks.try_join_next_with_id() {
      match result {
        Ok((id, result)) => {
          let origin = self.take_origin(id);
          completions.push(CommandCompletion {
            expected_server_instance_id: origin.expected_server_instance_id,
            model_setup_token: origin.model_setup_token,
            result,
            task_failure: false,
          });
        }
        Err(error) => {
          let origin = self.take_origin(error.id());
          completions.push(CommandCompletion {
            expected_server_instance_id: origin.expected_server_instance_id,
            model_setup_token: origin.model_setup_token,
            result: Err(AgentClientError::unavailable(
              AvailabilityKind::ShuttingDown,
              format!("command task failed: {error}"),
            )),
            task_failure: true,
          });
        }
      }
    }
    completions
  }

  pub fn drain_failures(&mut self) -> Vec<AgentClientError> {
    self
      .drain_completions()
      .into_iter()
      .filter_map(|completion| completion.result.err())
      .collect()
  }

  fn dispatch_with_origin(
    &mut self,
    client: Arc<dyn AgentClient>,
    expected_server_instance_id: String,
    command: AgentCommand,
    model_setup_token: Option<u64>,
  ) {
    let origin = CommandOrigin {
      expected_server_instance_id: expected_server_instance_id.clone(),
      model_setup_token,
    };
    let task = self.tasks.spawn(async move {
      client
        .execute_for_server(&expected_server_instance_id, command)
        .await
    });
    self.origins.insert(task.id(), origin);
  }

  fn take_origin(&mut self, id: Id) -> CommandOrigin {
    self
      .origins
      .remove(&id)
      .expect("every command task retains its dispatch origin")
  }
}

impl CommandCompletion {
  pub fn is_model_setup(&self) -> bool {
    self.model_setup_token.is_some()
  }

  pub fn model_setup_token(&self) -> Option<u64> {
    self.model_setup_token
  }

  pub fn expected_server_instance_id(&self) -> &str {
    &self.expected_server_instance_id
  }

  pub fn result(&self) -> &Result<(), AgentClientError> {
    &self.result
  }

  pub fn is_task_failure(&self) -> bool {
    self.task_failure
  }

  pub fn into_result(self) -> Result<(), AgentClientError> {
    self.result
  }
}

pub fn apply_command_completion(
  state: &mut crate::tui::TuiState,
  setup_commands: &mut super::ModelSetupCommandGate,
  completion: CommandCompletion,
) -> super::ModelSetupCompletion {
  if state.accepted_server_instance_id() != Some(completion.expected_server_instance_id()) {
    return super::ModelSetupCompletion::Continue;
  }
  let succeeded = completion.result().is_ok();
  let setup_result = completion
    .model_setup_token()
    .map_or(super::ModelSetupCompletion::Continue, |token| {
      setup_commands.complete(token, succeeded)
    });
  if let Err(error) = completion.into_result() {
    state.record_client_failure(error);
  }
  setup_result
}

pub(super) fn dispatch(
  runtime: &super::Runtime,
  commands: &mut CommandDispatcher,
  command: AgentCommand,
) -> AnyResult<()> {
  let client = runtime
    .client
    .as_ref()
    .context("application runtime is unavailable")?
    .clone();
  let expected = runtime
    .state
    .accepted_server_instance_id()
    .context("agent command requires an authoritative daemon epoch")?;
  commands.dispatch_for_server(client, expected.to_owned(), command);
  Ok(())
}

pub(super) fn dispatch_model_setup(
  runtime: &super::Runtime,
  commands: &mut CommandDispatcher,
  expected_server_instance_id: String,
  command: AgentCommand,
  token: u64,
) -> AnyResult<()> {
  let client = runtime
    .client
    .as_ref()
    .context("application runtime is unavailable")?
    .clone();
  commands.dispatch_model_setup(client, expected_server_instance_id, command, token);
  Ok(())
}

impl Default for CommandDispatcher {
  fn default() -> Self {
    Self::new()
  }
}
