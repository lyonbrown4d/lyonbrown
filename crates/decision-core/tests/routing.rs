mod support;
use decision_core::*;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use support::*;

#[tokio::test]
async fn explicit_and_pinned_models_are_never_silently_substituted() {
  for explicit in [true, false] {
    let mut request = route("Rust", vec![model("cloud", false), model("local", true)]);
    request.constraints.local_only = true;
    let Operation::RouteModel(input) = &mut request.operation else {
      unreachable!()
    };
    if explicit {
      input.explicit_model = Some("cloud".into());
    } else {
      input.pinned_model = Some("cloud".into());
    }
    let backend = Arc::new(Stub::output(DecisionOutput::Unknown));
    let result = decide(&request, vec![backend.clone()]).await;
    assert_eq!(result.status, DecisionStatus::Denied);
    assert_eq!(result.output, DecisionOutput::Unknown);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
  }
  let mut request = route("Rust", vec![model("local", true)]);
  let Operation::RouteModel(input) = &mut request.operation else {
    unreachable!()
  };
  input.explicit_model = Some("local".into());
  input.pinned_model = Some("missing-old-pin".into());
  let result = decide(&request, vec![]).await;
  assert_eq!(result.status, DecisionStatus::Accepted);
  let DecisionOutput::RouteModel(advice) = result.output else {
    panic!("route")
  };
  assert_eq!(advice.candidate_model_keys, ["local"]);
}

#[tokio::test]
async fn hard_constraints_apply_to_rule_and_fallback_routes() {
  for text in ["Rust code", "qzx"] {
    let mut models = vec![
      model("eligible", true),
      model("cloud", false),
      model("costly", true),
      model("small", true),
      model("without-tools", true),
      model("unapproved", true),
      model("wrong-tier", true),
    ];
    models[2].estimated_cost_microusd = Some(101);
    models[3].context_tokens = Some(20);
    models[4].capabilities.clear();
    models[6].tier = ModelTier::Advanced;
    let mut request = route(text, models);
    request.constraints = Constraints {
      local_only: true,
      allowed_tiers: [ModelTier::Balanced].into(),
      allowed_model_keys: Some(
        [
          "eligible",
          "cloud",
          "costly",
          "small",
          "without-tools",
          "wrong-tier",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
      ),
      required_capabilities: ["tools".into()].into(),
      input_tokens: Some(21),
      max_estimated_cost_microusd: Some(100),
    };
    let result = decide(&request, vec![]).await;
    assert!(matches!(
      result.status,
      DecisionStatus::Accepted | DecisionStatus::Degraded
    ));
    let DecisionOutput::RouteModel(advice) = result.output else {
      panic!("route")
    };
    assert_eq!(advice.candidate_model_keys, ["eligible"]);
    assert_eq!(advice.recommended_tier, ModelTier::Balanced);
  }
}

#[tokio::test]
async fn unknown_capacity_cost_or_empty_authorization_deny() {
  for variant in 0..4 {
    let mut candidate = model("local", true);
    let mut request = route("Rust", vec![]);
    match variant {
      0 => {
        candidate.context_tokens = None;
        request.constraints.input_tokens = Some(1);
      }
      1 => {
        candidate.estimated_cost_microusd = None;
        request.constraints.max_estimated_cost_microusd = Some(100);
      }
      2 => request.constraints.allowed_model_keys = Some(Default::default()),
      _ => request.constraints.local_only = true,
    }
    if variant != 3 {
      let Operation::RouteModel(input) = &mut request.operation else {
        unreachable!()
      };
      input.candidates.push(candidate);
    }
    assert_eq!(
      decide(&request, vec![]).await.status,
      DecisionStatus::Denied
    );
  }
}

#[tokio::test]
async fn backend_cannot_expand_model_scope_or_relax_local_only() {
  let mut request = route("qzx", vec![model("local", true), model("cloud", false)]);
  request.constraints.local_only = true;
  let output = DecisionOutput::RouteModel(RouteAdvice {
    complexity: Complexity::High,
    task_domain: "coding".into(),
    recommended_tier: ModelTier::Balanced,
    required_capabilities: Default::default(),
    candidate_model_keys: vec!["cloud".into()],
  });
  let result = decide(&request, vec![Arc::new(Stub::output(output))]).await;
  assert_eq!(result.status, DecisionStatus::Degraded);
  assert!(
    result
      .provenance
      .reason_codes
      .iter()
      .any(|v| v == "INVALID_BACKEND_OUTPUT")
  );
  let DecisionOutput::RouteModel(advice) = result.output else {
    panic!("route")
  };
  assert_eq!(advice.candidate_model_keys, ["local"]);
  let mut remote = Stub::output(DecisionOutput::Unknown);
  remote.local = false;
  let remote = Arc::new(remote);
  let result = decide(&request, vec![remote.clone()]).await;
  assert_eq!(remote.calls.load(Ordering::SeqCst), 0);
  assert_eq!(result.status, DecisionStatus::Degraded);
}

#[tokio::test]
async fn unknown_and_ambiguous_inputs_do_not_claim_classification() {
  for text in ["qzx", "Rust 翻译", "decode"] {
    let result = decide(&route(text, vec![]), vec![]).await;
    assert_eq!(result.status, DecisionStatus::Unknown);
    assert_eq!(result.output, DecisionOutput::Unknown);
  }
}
