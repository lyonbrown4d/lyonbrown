use crate::{Gateway, GatewayError, ModelConfig, request::ValidatedRequest};
use decision_core::{
  Constraints, DecisionCancellation, DecisionOutput, DecisionRequest, DecisionStatus,
  ModelCandidate, Operation, RouteInput,
};

impl Gateway {
  pub(crate) async fn select(
    &self,
    request_id: &str,
    request: &ValidatedRequest,
    deadline: tokio::time::Instant,
  ) -> Result<(ModelConfig, String), GatewayError> {
    if tokio::time::Instant::now() >= deadline {
      return Err(GatewayError::routing_timeout());
    }
    let mut constraints = self.inner.config.constraints.clone();
    constraints.local_only |= request.routing.local_only;
    if !request.routing.allowed_tiers.is_empty() {
      if constraints.allowed_tiers.is_empty() {
        constraints.allowed_tiers = request.routing.allowed_tiers.clone();
      } else {
        constraints.allowed_tiers = constraints
          .allowed_tiers
          .intersection(&request.routing.allowed_tiers)
          .copied()
          .collect();
        if constraints.allowed_tiers.is_empty() {
          return Err(no_model());
        }
      }
    }
    constraints.required_capabilities.insert("text".into());
    if request.tools
      || request.body["messages"].as_array().is_some_and(|messages| {
        messages
          .iter()
          .any(|message| message.get("tool_calls").is_some() || message["role"] == "tool")
      })
    {
      constraints.required_capabilities.insert("tools".into());
    }
    constraints.input_tokens = Some(
      constraints
        .input_tokens
        .unwrap_or(0)
        .max(request.context_tokens),
    );
    let eligible: Vec<_> = self
      .inner
      .config
      .models
      .iter()
      .filter(|model| model_eligible(model, &constraints))
      .collect();
    if eligible.is_empty() {
      return Err(no_model());
    }
    if !matches!(request.requested_model.as_str(), "auto" | "router/auto") {
      return eligible
        .into_iter()
        .find(|model| model.id == request.requested_model)
        .cloned()
        .map(|model| {
          (
            model,
            self.inner.decision.snapshot().policy().revision.clone(),
          )
        })
        .ok_or_else(no_model);
    }
    let mut decision_request = DecisionRequest::new(
      request_id,
      Operation::RouteModel(RouteInput {
        text: request.text.clone(),
        candidates: eligible
          .iter()
          .map(|model| ModelCandidate {
            key: model.id.clone(),
            tier: model.tier,
            local: model.local_inference,
            capabilities: model.capabilities.clone(),
            context_tokens: model.context_tokens,
            estimated_cost_microusd: None,
          })
          .collect(),
        explicit_model: None,
        pinned_model: None,
      }),
    );
    decision_request.constraints = constraints.clone();
    decision_request.deadline_ms = deadline
      .saturating_duration_since(tokio::time::Instant::now())
      .as_millis()
      .min(u128::from(u64::MAX)) as u64;
    if decision_request.deadline_ms == 0 {
      return Err(GatewayError::routing_timeout());
    }
    let result = self
      .inner
      .decision
      .decide(&decision_request, &DecisionCancellation::new())
      .await;
    if tokio::time::Instant::now() >= deadline {
      return Err(GatewayError::routing_timeout());
    }
    if !matches!(
      result.status,
      DecisionStatus::Accepted | DecisionStatus::Degraded
    ) {
      if result
        .provenance
        .reason_codes
        .iter()
        .any(|code| matches!(code.as_str(), "DEADLINE_EXCEEDED" | "BACKEND_TIMEOUT"))
      {
        return Err(GatewayError::routing_timeout());
      }
      return Err(no_model());
    }
    if let DecisionOutput::RouteModel(advice) = result.output {
      constraints
        .required_capabilities
        .extend(advice.required_capabilities);
      for key in advice.candidate_model_keys {
        if let Some(model) = eligible.iter().find(|model| {
          model.id == key
            && model.tier == advice.recommended_tier
            && model_eligible(model, &constraints)
        }) {
          return Ok(((*model).clone(), result.provenance.policy_revision));
        }
      }
    }
    Err(no_model())
  }
}
fn model_eligible(model: &ModelConfig, c: &Constraints) -> bool {
  (!c.local_only || model.local_inference)
    && (c.allowed_tiers.is_empty() || c.allowed_tiers.contains(&model.tier))
    && c
      .allowed_model_keys
      .as_ref()
      .is_none_or(|keys| keys.contains(&model.id))
    && c.required_capabilities.is_subset(&model.capabilities)
    && c.input_tokens.is_none_or(|tokens| {
      model
        .context_tokens
        .is_some_and(|capacity| capacity >= tokens)
    })
    && c.max_estimated_cost_microusd.is_none()
}
fn no_model() -> GatewayError {
  GatewayError(
    axum::http::StatusCode::NOT_FOUND,
    "no_eligible_model",
    "No configured model satisfies this request",
  )
}
