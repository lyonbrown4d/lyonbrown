use crate::{
  Result, Runtime, RuntimeError, RuntimeEvent, ToolCall, ToolOutput,
  persistence::{ToolIntent, active, rig_error},
};
use agent_core::events::{AgentEvent, ExecutionPhase};
use rig_agent::run::PendingToolCall;
use rig_core::{
  completion::ToolDefinition,
  message::ToolName,
  tool::{ToolExecutionError, ToolOutput as RigOutput, ToolResult},
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
impl Runtime {
  pub(crate) fn tool_definitions(&self) -> Result<Vec<Arc<ToolDefinition>>> {
    self
      .tools
      .values()
      .map(|tool| {
        let definition = tool.definition();
        Ok(Arc::new(ToolDefinition {
          name: ToolName::new(definition.name).map_err(rig_error)?,
          description: definition.description,
          parameters: definition.parameters,
          annotations: Default::default(),
        }))
      })
      .collect()
  }
  pub(crate) async fn call_tool(
    &self,
    call: PendingToolCall,
    cancellation: &CancellationToken,
  ) -> Result<()> {
    let call = match call {
      PendingToolCall::Malformed(call) => {
        return self
          .session
          .update("malformed_tool_answered", |state| {
            active(state)?
              .run
              .answer(call.answer(None))
              .map_err(rig_error)?;
            Ok(((), json!({}), vec![]))
          })
          .await;
      }
      PendingToolCall::Execute(call) => call,
      _ => {
        return Err(RuntimeError::Execution(
          "unsupported Rig tool call kind".into(),
        ));
      }
    };
    let id = call.id().to_string();
    let name = call.name().to_string();
    let arguments = call.arguments();
    // The selected tool remains a host-authorized port. Rig's policy never
    // grants capabilities or constructs a tool that was not registered here.
    let tool = self
      .tools
      .get(&name)
      .ok_or_else(|| RuntimeError::Tool("unregistered tool".into()))?;
    self
      .session
      .update("tool_started", |state| {
        if !state.pending_tools.is_empty() {
          return Err(RuntimeError::Uncertain(
            "pending tool outcome requires reconciliation".into(),
          ));
        }
        state.pending_tools.insert(
          id.clone(),
          ToolIntent {
            call_id: id.clone(),
            name: name.clone(),
            arguments: arguments.clone(),
          },
        );
        Ok((
          (),
          json!({"call_id":id,"name":name}),
          vec![
            AgentEvent::PhaseChanged {
              phase: ExecutionPhase::PreparingTool,
              model: None,
            },
            AgentEvent::ToolStarted {
              call_id: id.clone(),
              name: name.clone(),
              arguments: arguments.clone(),
            },
          ],
        ))
      })
      .await?;
    (self.listener)(RuntimeEvent::PreparingTool);
    (self.listener)(RuntimeEvent::ToolStarted {
      call_id: id.clone(),
      name: name.clone(),
      arguments: arguments.clone(),
    });
    let token = cancellation.child_token();
    let execution = tool.execute(
      ToolCall {
        id: id.clone(),
        arguments,
      },
      token.clone(),
    );
    let output = tokio::select! { biased;
      _ = self.session.stopped.cancelled() => return Err(RuntimeError::Uncertain("session writer fenced during tool execution".into())),
      _ = cancellation.cancelled() => {
        token.cancel();
        self.session.uncertain(format!("tool {name} ({id}) cancelled with an unconfirmed outcome")).await?;
        return Err(RuntimeError::Uncertain("tool cancellation outcome unresolved".into()));
      },
      output = tokio::time::timeout(Duration::from_secs(300), execution) => match output {
        Ok(output) => output,
        Err(_) => {
          token.cancel();
          self.session.uncertain(format!("tool {name} ({id}) timed out with an unconfirmed outcome")).await?;
          return Err(RuntimeError::Uncertain("tool timeout outcome unresolved".into()));
        }
      }
    };
    let output = match output {
      Ok(output) => output,
      Err(RuntimeError::Uncertain(message)) => {
        self.session.uncertain(message).await?;
        return Err(RuntimeError::Uncertain("tool outcome unresolved".into()));
      }
      Err(error) => ToolOutput {
        text: error.to_string(),
        is_error: true,
      },
    };
    let output = if output.text.len() > crate::budget::TOOL_BYTES {
      ToolOutput {
        text: format!(
          "The tool returned a confirmed {} result. The result body was omitted because it exceeded the 16MiB host limit. Do not repeat the operation merely to regenerate the omitted body.",
          if output.is_error {
            "error"
          } else {
            "successful"
          }
        ),
        is_error: output.is_error,
      }
    } else {
      output
    };
    let result = if output.is_error {
      ToolResult::failed(ToolExecutionError::other(output.text.clone()))
    } else {
      ToolResult::success(RigOutput::text(output.text.clone()))
    };
    self
      .session
      .update("tool_result_committed", |state| {
        active(state)?
          .run
          .answer(call.answer(result))
          .map_err(rig_error)?;
        state.pending_tools.remove(&id);
        Ok((
          (),
          json!({"call_id":id,"name":name}),
          vec![AgentEvent::ToolFinished {
            call_id: id.clone(),
            name: name.clone(),
            text: output.text.clone(),
            is_error: output.is_error,
          }],
        ))
      })
      .await?;
    (self.listener)(RuntimeEvent::ToolFinished {
      call_id: id,
      name,
      text: output.text,
      is_error: output.is_error,
    });
    Ok(())
  }
}
