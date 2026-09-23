use super::*;

pub(super) fn serialized_system_tokens(
  estimator: &TokenEstimator,
  system: &str,
) -> Result<u64, AgentError> {
  serde_json::to_string(&Some(system))
    .map(|serialized| estimator.estimate(&serialized))
    .map_err(|error| AgentError::Context(format!("could not serialize system prompt: {error}")))
}

pub(super) fn serialized_context_tokens(
  estimator: &TokenEstimator,
  messages: &[ChatMessage],
  tools: &[agent_core::ToolDefinition],
) -> Result<u64, AgentError> {
  serde_json::to_string(&(messages, tools))
    .map(|serialized| estimator.estimate(&serialized))
    .map_err(|error| AgentError::Context(format!("could not serialize request context: {error}")))
}
