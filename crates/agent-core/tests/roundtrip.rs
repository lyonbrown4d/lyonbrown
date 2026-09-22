//! M0 验收：每个 AgentEvent 变体 serde round-trip；ContextItem 优先级排序。

use agent_core::{
    AgentEvent, ContextItem, ContextItemKind, ObservationId, RiskLevel, SecretRef, SessionId,
    StepId, StepOutcome, StopReason, TokenUsage, ToolCallId, ToolDefinition, ToolInvocation,
    ToolResult, ToolSource, TurnId,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

fn sample_events() -> Vec<AgentEvent> {
    let sid = Uuid::new_v4();
    let tid = Uuid::new_v4();
    let stid = Uuid::new_v4();
    let oid = Uuid::new_v4();
    let cid = Uuid::new_v4();
    let ts = Utc::now();
    let inv = ToolInvocation {
        id: ToolCallId(cid),
        name: "echo".into(),
        arguments: json!({"text": "hi"}),
    };
    vec![
        AgentEvent::SessionStarted {
            session_id: SessionId(sid),
            ts,
        },
        AgentEvent::UserMessageReceived {
            session_id: SessionId(sid),
            turn_id: TurnId(tid),
            text: "hello".into(),
            ts,
        },
        AgentEvent::TurnStarted {
            session_id: SessionId(sid),
            turn_id: TurnId(tid),
            ts,
        },
        AgentEvent::StepStarted {
            session_id: SessionId(sid),
            turn_id: TurnId(tid),
            step_id: StepId(stid),
            ts,
        },
        AgentEvent::ContextPrepared {
            step_id: StepId(stid),
            system_tokens: 100,
            context_tokens: 50,
            dropped: 0,
            ts,
        },
        AgentEvent::ModelSelected {
            step_id: StepId(stid),
            provider: "ollama".into(),
            model: "qwen".into(),
            reason: "worker".into(),
            ts,
        },
        AgentEvent::ModelStarted {
            step_id: StepId(stid),
            ts,
        },
        AgentEvent::ModelDelta {
            step_id: StepId(stid),
            delta: "he".into(),
        },
        AgentEvent::ModelCompleted {
            step_id: StepId(stid),
            usage: TokenUsage {
                input: 10,
                output: 5,
                cache_read: 0,
            },
            ts,
        },
        AgentEvent::ToolRequested {
            step_id: StepId(stid),
            invocation: inv.clone(),
            ts,
        },
        AgentEvent::ToolApproved {
            step_id: StepId(stid),
            tool_call_id: ToolCallId(cid),
            by: "user".into(),
            ts,
        },
        AgentEvent::ToolStarted {
            step_id: StepId(stid),
            tool_call_id: ToolCallId(cid),
            ts,
        },
        AgentEvent::ToolCompleted {
            step_id: StepId(stid),
            result: ToolResult {
                invocation_id: ToolCallId(cid),
                ok: true,
                output: "hi".into(),
                truncated: false,
                duration_ms: 3,
                error: None,
            },
            ts,
        },
        AgentEvent::ObservationCreated {
            step_id: StepId(stid),
            observation_id: ObservationId(oid),
            ts,
        },
        AgentEvent::StepCompleted {
            step_id: StepId(stid),
            outcome: StepOutcome {
                stop_reason: StopReason::ToolUse,
                text: "".into(),
                tool_calls: vec![ToolCallId(cid)],
                escalated: false,
            },
            ts,
        },
        AgentEvent::TurnCompleted {
            turn_id: TurnId(tid),
            answer: "done".into(),
            ts,
        },
        AgentEvent::SessionClosed {
            session_id: SessionId(sid),
            ts,
        },
        AgentEvent::Error {
            session_id: SessionId(sid),
            message: "boom".into(),
            ts,
        },
    ]
}

#[test]
fn every_event_variant_roundtrips() {
    for ev in sample_events() {
        let s = serde_json::to_string(&ev).unwrap();
        let back: AgentEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(ev, back, "round-trip mismatch for {}", ev.kind());
        // tag 字段即变体名
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["type"], ev.kind());
    }
}

#[test]
fn context_item_priority_ordering() {
    let mut items: Vec<ContextItem> = (0..5)
        .map(|i| {
            let mut c = ContextItem::new(ContextItemKind::Message, format!("msg {i}"), (4 - i) as u8);
            c.source = format!("src{i}");
            c
        })
        .collect();
    // priority 升序、timestamp 升序
    items.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.timestamp.cmp(&b.timestamp))
    });
    let prios: Vec<u8> = items.iter().map(|c| c.priority).collect();
    assert_eq!(prios, vec![0, 1, 2, 3, 4]);
}

#[test]
fn secret_ref_parts() {
    assert_eq!(SecretRef::env("ANTHROPIC_API_KEY").parts(), ("env", "ANTHROPIC_API_KEY"));
    assert_eq!(SecretRef::file("github").parts(), ("file", "github"));
}

#[test]
fn tool_definition_serde() {
    let def = ToolDefinition {
        name: "echo".into(),
        description: "echo".into(),
        input_schema: json!({"type": "object"}),
        source: ToolSource::BuiltIn,
        risk: RiskLevel::Execute,
        requires_secrets: vec![SecretRef::env("X")],
    };
    let s = serde_json::to_string(&def).unwrap();
    let back: ToolDefinition = serde_json::from_str(&s).unwrap();
    assert_eq!(def, back);
}
