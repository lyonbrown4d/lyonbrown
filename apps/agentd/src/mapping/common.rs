use agent_app::{ModelRoute, RouteSummary};
use agent_core::{
  RiskLevel as AppRisk, SandboxStatus as AppSandbox, StepOutcome as AppOutcome,
  StopReason as AppStopReason, ToolPreview as AppPreview, ToolResult as AppResult,
};
use agent_protocol::v1::{
  ModelFallback, RiskLevel, SandboxStatus, StepOutcome, StopReason, StopReasonKind, ToolPreview,
  ToolResult,
};

pub(super) fn route_to_wire(route: RouteSummary) -> agent_protocol::v1::RouteSummary {
  agent_protocol::v1::RouteSummary {
    selected: Some(model_route_to_wire(route.selected)),
    reason: route.reason,
    fallbacks: route
      .fallbacks
      .into_iter()
      .map(|fallback| ModelFallback {
        from: Some(model_route_to_wire(fallback.from)),
        to: Some(model_route_to_wire(fallback.to)),
        reason: fallback.reason,
      })
      .collect(),
  }
}

pub(super) fn model_route_to_wire(route: ModelRoute) -> agent_protocol::v1::ModelRoute {
  agent_protocol::v1::ModelRoute {
    provider: route.provider,
    model: route.model,
  }
}

pub(super) fn tool_preview_to_wire(preview: AppPreview) -> ToolPreview {
  ToolPreview {
    tool: preview.tool,
    action: preview.action,
    target: preview.target,
    impact: preview.impact,
    risk: risk_to_wire(preview.risk) as i32,
    sandbox: sandbox_to_wire(preview.sandbox) as i32,
    working_directory: preview
      .working_directory
      .map(|path| path.to_string_lossy().into_owned()),
    required_secrets: preview
      .required_secrets
      .into_iter()
      .map(|secret| secret.0)
      .collect(),
  }
}

pub(super) fn tool_result_to_wire(result: AppResult) -> ToolResult {
  ToolResult {
    invocation_id: result.invocation_id.to_string(),
    ok: result.ok,
    output: result.output,
    truncated: result.truncated,
    duration_ms: result.duration_ms,
    error: result.error,
  }
}

pub(super) fn step_outcome_to_wire(outcome: AppOutcome) -> StepOutcome {
  StepOutcome {
    stop_reason: Some(stop_reason_to_wire(outcome.stop_reason)),
    text: outcome.text,
    tool_call_ids: outcome
      .tool_calls
      .into_iter()
      .map(|id| id.to_string())
      .collect(),
    escalated: outcome.escalated,
  }
}

fn stop_reason_to_wire(reason: AppStopReason) -> StopReason {
  let (kind, error) = match reason {
    AppStopReason::EndTurn => (StopReasonKind::EndTurn, None),
    AppStopReason::ToolUse => (StopReasonKind::ToolUse, None),
    AppStopReason::MaxTokens => (StopReasonKind::MaxTokens, None),
    AppStopReason::Stop => (StopReasonKind::Stop, None),
    AppStopReason::Error(error) => (StopReasonKind::Error, Some(error)),
  };
  StopReason {
    kind: kind as i32,
    error,
  }
}

fn risk_to_wire(risk: AppRisk) -> RiskLevel {
  match risk {
    AppRisk::Read => RiskLevel::Read,
    AppRisk::Write => RiskLevel::Write,
    AppRisk::Execute => RiskLevel::Execute,
    AppRisk::Network => RiskLevel::Network,
  }
}

fn sandbox_to_wire(status: AppSandbox) -> SandboxStatus {
  match status {
    AppSandbox::WorkspaceRestricted => SandboxStatus::WorkspaceRestricted,
    AppSandbox::HostUnsandboxed => SandboxStatus::HostUnsandboxed,
    AppSandbox::Unknown => SandboxStatus::Unknown,
  }
}
