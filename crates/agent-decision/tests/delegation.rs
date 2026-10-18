#[allow(dead_code)]
mod support;
use agent_decision::*;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn local_or_unavailable_target_plans_cannot_expose_delegation_on_any_protocol() {
  for protocol in StandardProtocol::ALL {
    for no_targets in [false, true] {
      let mut policy = support::policy();
      policy.authorized_tools.insert(Tool::DelegateTask);
      policy.available_capabilities.insert(Capability::Delegation);
      let mut candidate = support::candidate("code", &["read_file", "delegate_task"]);
      if no_targets {
        candidate["execution_mode"] = "distributed_allowed".into();
        candidate["target_hint"] = "paired-worker".into();
      }
      let server = MockModelServer::start(vec![text_reply(protocol, &candidate.to_string())])
        .await
        .expect("local classifier fixture");
      let engine = support::engine(&server, protocol, policy);
      let engine = if no_targets {
        // A newly observed unconfigured peer must not become authorized.
        engine.with_available_targets(&["unconfigured-peer".into()].into())
      } else {
        engine.with_available_targets(&["paired-worker".into()].into())
      };
      let plan = engine
        .decide("classify only", None, CancellationToken::new())
        .await
        .unwrap();
      assert_eq!(plan.execution_mode, ExecutionMode::Local);
      assert!(!plan.allowed_tools.contains(&Tool::DelegateTask));
      assert!(!plan.required_capabilities.contains(&Capability::Delegation));
      assert!(plan.allowed_tools.contains(&Tool::ReadFile));
      assert!(plan.target_hint.is_none());
      assert_eq!(plan.source, DecisionSource::Classifier { restricted: true });
      assert_eq!(server.requests().await.len(), 1);
    }
  }
}

#[tokio::test]
async fn explicit_local_route_cannot_grant_the_delegation_tool() {
  let mut policy = support::policy();
  policy.authorized_tools.insert(Tool::DelegateTask);
  policy.available_capabilities.insert(Capability::Delegation);
  let engine = DecisionEngine::new(policy, None).unwrap();
  let route = ExplicitRoute {
    task_kind: TaskKind::Code,
    model_policy: ModelPolicy::General,
    tools: [Tool::DelegateTask].into(),
    execution_mode: ExecutionMode::Local,
    target_hint: None,
  };
  assert!(matches!(
    engine
      .decide("delegate", Some(route), CancellationToken::new())
      .await,
    Err(DecisionError::ForbiddenRoute)
  ));
}
