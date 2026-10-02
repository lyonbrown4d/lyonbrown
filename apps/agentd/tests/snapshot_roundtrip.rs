use std::path::PathBuf;
use std::time::{Duration, Instant};

use agent_app::{
  ActivityPhase, ActivityView, ApprovalState, ApprovalView, ContextSummary, ManagedModelView,
  ModelFallbackView, ModelRoute, ModelSelection, ModelSetupAction, ModelSetupPhase,
  ModelSetupSource, ModelSetupView, RouteSummary, RuntimePhase, RuntimeSnapshot, ToolActivityState,
  ToolActivityView, TurnStatus, TurnView,
};
use agent_client::decode_snapshot;
use agent_core::{
  RiskLevel, SandboxStatus, SecretRef, SessionId, StepId, StepOutcome, StopReason, TokenUsage,
  ToolCallId, ToolPreview, ToolResult, TurnId,
};
use agentd::{VersionedSnapshot, snapshot_to_wire};
use serde_json::json;
use uuid::Uuid;

fn id(value: u128) -> Uuid {
  Uuid::from_u128(value)
}

fn route() -> RouteSummary {
  RouteSummary {
    selected: ModelRoute {
      provider: "local".into(),
      model: "worker".into(),
    },
    reason: "best fit".into(),
    fallbacks: vec![ModelFallbackView {
      from: ModelRoute {
        provider: "local".into(),
        model: "small".into(),
      },
      to: ModelRoute {
        provider: "local".into(),
        model: "worker".into(),
      },
      reason: "tools required".into(),
    }],
  }
}

fn full_snapshot() -> RuntimeSnapshot {
  let call_id = ToolCallId(id(4));
  RuntimeSnapshot {
    phase: RuntimePhase::AwaitingApproval,
    session_id: Some(SessionId(id(1))),
    turns: vec![TurnView {
      turn_id: TurnId(id(2)),
      user_text: "build it".into(),
      steering: vec!["focus tests".into()],
      assistant_text: "working".into(),
      status: TurnStatus::Running,
      activities: vec![ActivityView {
        step_id: StepId(id(3)),
        phase: ActivityPhase::RunningTool,
        route: Some(route()),
        context: Some(ContextSummary {
          system_tokens: 11,
          context_tokens: 22,
          dropped: 3,
        }),
        assistant_text: "using tool".into(),
        usage: Some(TokenUsage {
          input: 101,
          output: 55,
          cache_read: 8,
        }),
        tools: vec![ToolActivityView {
          call_id,
          name: "shell".into(),
          arguments: json!({"command": "cargo test"}),
          state: ToolActivityState::Completed,
          approved_by: Some("user".into()),
          result: Some(ToolResult {
            invocation_id: call_id,
            ok: true,
            output: "ok".into(),
            truncated: false,
            duration_ms: 42,
            error: None,
          }),
        }],
        outcome: Some(StepOutcome {
          stop_reason: StopReason::ToolUse,
          text: "done".into(),
          tool_calls: vec![call_id],
          escalated: false,
        }),
      }],
    }],
    approval: Some(ApprovalView {
      request_id: call_id,
      step_id: StepId(id(3)),
      tool_name: "shell".into(),
      arguments: json!({"command": "cargo test"}),
      preview: Some(ToolPreview {
        tool: "shell".into(),
        action: "execute".into(),
        target: "workspace".into(),
        impact: "runs tests".into(),
        risk: RiskLevel::Execute,
        sandbox: SandboxStatus::WorkspaceRestricted,
        working_directory: Some(PathBuf::from("project")),
        required_secrets: vec![SecretRef::env("TOKEN")],
      }),
      deadline: None,
      state: ApprovalState::Approved,
      approved_by: Some("user".into()),
    }),
    queued_follow_ups: vec!["document it".into()],
    model_selection: ModelSelection::Pinned {
      provider: "local".into(),
      model: "worker".into(),
    },
    route: Some(route()),
    last_error: Some("recoverable".into()),
    model_setup: Some(ModelSetupView {
      source: ModelSetupSource::ManagedPreset,
      phase: ModelSetupPhase::Downloading,
      managed_model: Some(ManagedModelView {
        id: "qwen3-1.7b-q4_k_m".into(),
        expected_download_bytes: 1_000,
        max_bytes: 1_100,
        min_available_memory_bytes: 2_000,
        sha256: "ab".repeat(32),
      }),
      operation_id: Some("download-1".into()),
      downloaded_bytes: 400,
      total_bytes: Some(1_000),
      failure: None,
      allowed_actions: vec![ModelSetupAction::Cancel],
    }),
  }
}

#[test]
fn full_runtime_snapshot_roundtrips_without_losing_ui_state() {
  let expected = full_snapshot();
  let wire = snapshot_to_wire(VersionedSnapshot::new(17, expected.clone()));

  let decoded = decode_snapshot(wire).unwrap();

  assert_eq!(decoded.revision, 17);
  assert_eq!(decoded.snapshot, expected);
}

#[test]
fn approval_deadline_roundtrips_as_remaining_duration() {
  let mut expected = full_snapshot();
  expected.approval.as_mut().unwrap().deadline = Some(Instant::now() + Duration::from_secs(5));

  let wire = snapshot_to_wire(VersionedSnapshot::new(18, expected));
  let decoded = decode_snapshot(wire).unwrap();
  let remaining = decoded
    .snapshot
    .approval
    .unwrap()
    .deadline
    .unwrap()
    .saturating_duration_since(Instant::now());

  assert!(remaining <= Duration::from_secs(5));
  assert!(remaining >= Duration::from_secs(4));
}

#[test]
fn unknown_snapshot_enum_is_a_protocol_error() {
  let mut wire = snapshot_to_wire(VersionedSnapshot::new(19, RuntimeSnapshot::default()));
  wire.phase = 999;

  let error = decode_snapshot(wire).unwrap_err();

  assert!(matches!(
    error,
    agent_client::SnapshotDecodeError::UnknownEnum {
      field: "phase",
      value: 999
    }
  ));
}
