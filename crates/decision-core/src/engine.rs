use crate::{rules, validation, *};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Thread-safe embedded decision orchestration. It owns no model dispatch or durable state.
pub struct DecisionEngine {
  policy: RwLock<Arc<PolicySnapshot>>,
  backends: Vec<Arc<dyn DecisionBackend>>,
}
impl DecisionEngine {
  pub fn new(policy: Policy, backends: Vec<Arc<dyn DecisionBackend>>) -> Result<Self, PolicyError> {
    if backends.len() > 16 {
      return Err(PolicyError("at most 16 backends are permitted"));
    }
    Ok(Self {
      policy: RwLock::new(Arc::new(PolicySnapshot::new(policy)?)),
      backends,
    })
  }
  pub fn snapshot(&self) -> Arc<PolicySnapshot> {
    Arc::clone(
      &self
        .policy
        .read()
        .unwrap_or_else(|poison| poison.into_inner()),
    )
  }
  pub fn replace_policy(&self, policy: Policy) -> Result<(), PolicyError> {
    let replacement = Arc::new(PolicySnapshot::new(policy)?);
    let mut current = self
      .policy
      .write()
      .unwrap_or_else(|poison| poison.into_inner());
    if current.policy().revision == replacement.policy().revision
      && current.policy() != replacement.policy()
    {
      return Err(PolicyError("changed policy requires a new revision"));
    }
    *current = replacement;
    Ok(())
  }
  pub async fn decide(
    &self,
    request: &DecisionRequest,
    cancellation: &DecisionCancellation,
  ) -> DecisionResult {
    let started = Instant::now();
    let snapshot = self.snapshot();
    let policy = snapshot.policy();
    let mut result = DecisionResult {
      request_id: if request.request_id.len() <= 128 {
        request.request_id.clone()
      } else {
        String::new()
      },
      status: DecisionStatus::Unknown,
      output: DecisionOutput::Unknown,
      provenance: Provenance {
        policy_id: policy.id.clone(),
        policy_revision: policy.revision.clone(),
        method: "rules".into(),
        backend_version: None,
        index_revision: None,
        reason_codes: vec![],
      },
      elapsed_ms: 0,
    };
    if cancellation.is_cancelled() {
      return finish(result, started, "CANCELLED");
    }
    if !validation::request_valid(request, policy) {
      result.status = DecisionStatus::Denied;
      return finish(result, started, "INVALID_REQUEST");
    }
    if request
      .policy_revision
      .as_ref()
      .is_some_and(|revision| revision != &policy.revision)
    {
      result.status = DecisionStatus::Denied;
      return finish(result, started, "POLICY_REVISION_MISMATCH");
    }
    if route_denied(request) {
      result.status = DecisionStatus::Denied;
      return finish(result, started, "NO_ELIGIBLE_ROUTE");
    }
    if let Some(output) = rules::evaluate(request, policy)
      && validation::output_valid(request, &output, policy)
    {
      if cancellation.is_cancelled() {
        return finish(result, started, "CANCELLED");
      }
      if started.elapsed().as_millis()
        >= u128::from(request.deadline_ms.min(policy.max_deadline_ms))
      {
        return finish(result, started, "DEADLINE_EXCEEDED");
      }
      result.provenance.reason_codes.push(
        match &request.operation {
          Operation::RouteModel(input) if input.explicit_model.is_some() => {
            "EXPLICIT_MODEL_VALIDATED"
          }
          Operation::RouteModel(input) if input.pinned_model.is_some() => "PINNED_MODEL_VALIDATED",
          Operation::RouteModel(_) | Operation::Classify(_) => "DOMAIN_TERM_MATCH",
          Operation::ContextAdvice(_) => "CONTEXT_FACTS",
          Operation::RecommendTools(_) => "AUTHORIZED_TOOL_TERM_MATCH",
          _ => "RULE_EVIDENCE",
        }
        .into(),
      );
      result.status = DecisionStatus::Accepted;
      result.output = output;
      return finish(result, started, "RULE_MATCH");
    }
    let remaining =
      std::time::Duration::from_millis(request.deadline_ms.min(policy.max_deadline_ms))
        .saturating_sub(started.elapsed());
    if remaining.is_zero() {
      return finish(result, started, "DEADLINE_EXCEEDED");
    }
    let deadline = tokio::time::Instant::now() + remaining;
    let mut degraded = false;
    for backend in &self.backends {
      if !backend.supports(&request.operation) {
        continue;
      }
      let descriptor = backend.descriptor();
      if request.constraints.local_only && !descriptor.local {
        degraded = true;
        result
          .provenance
          .reason_codes
          .push("REMOTE_BACKEND_EXCLUDED".into());
        continue;
      }
      let backend_cancellation = DecisionCancellation::new();
      let evaluated = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
          backend_cancellation.cancel();
          return finish(result, started, "CANCELLED");
        }
        evaluated = tokio::time::timeout_at(deadline, backend.evaluate(request, &snapshot, &backend_cancellation)) => evaluated,
      };
      backend_cancellation.cancel();
      if cancellation.is_cancelled() {
        return finish(result, started, "CANCELLED");
      }
      match evaluated {
        Err(_) => {
          result
            .provenance
            .reason_codes
            .push("BACKEND_TIMEOUT".into());
          degraded = true;
          break;
        }
        Ok(Err(_)) => {
          result
            .provenance
            .reason_codes
            .push("BACKEND_UNAVAILABLE".into());
          degraded = true;
        }
        Ok(Ok(decision)) => {
          if !validation::backend_metadata_valid(&decision, &descriptor) {
            result
              .provenance
              .reason_codes
              .push("INVALID_BACKEND_OUTPUT".into());
            degraded = true;
            continue;
          }
          if decision.output == DecisionOutput::Unknown {
            result.provenance.method = format!("{}:unknown", descriptor.name);
            result.provenance.backend_version = Some(descriptor.version);
            result.provenance.index_revision = descriptor.index_revision;
            result.provenance.reason_codes.extend(decision.reason_codes);
            continue;
          }
          if decision.confidence < policy.minimum_confidence {
            result.provenance.reason_codes.extend(decision.reason_codes);
            result.provenance.reason_codes.push("LOW_CONFIDENCE".into());
            degraded = true;
            continue;
          }
          if matches!(&decision.output, DecisionOutput::RouteModel(advice) if advice.task_domain == "unknown")
            || !validation::output_valid(request, &decision.output, policy)
          {
            result
              .provenance
              .reason_codes
              .push("INVALID_BACKEND_OUTPUT".into());
            degraded = true;
            continue;
          }
          result.status = if degraded {
            DecisionStatus::Degraded
          } else {
            DecisionStatus::Accepted
          };
          result.output = decision.output;
          result.provenance.method = descriptor.name;
          result.provenance.backend_version = Some(descriptor.version);
          result.provenance.index_revision = descriptor.index_revision;
          result.provenance.reason_codes.extend(decision.reason_codes);
          return finish(result, started, "BACKEND_MATCH");
        }
      }
    }
    if cancellation.is_cancelled() {
      return finish(result, started, "CANCELLED");
    }
    // A conservative route still requires a caller-authorized eligible catalog. No catalog is Unknown.
    if let Operation::RouteModel(input) = &request.operation
      && !input.candidates.is_empty()
      && let Some(output) = rules::route(request, policy, "unknown", policy.default_tier)
      && validation::output_valid(request, &output, policy)
    {
      result.status = DecisionStatus::Degraded;
      result.output = output;
      return finish(result, started, "CONSTRAINED_FALLBACK");
    }
    finish(
      result,
      started,
      if degraded {
        "BACKENDS_EXHAUSTED"
      } else {
        "NO_EVIDENCE"
      },
    )
  }
}
fn route_denied(request: &DecisionRequest) -> bool {
  let Operation::RouteModel(input) = &request.operation else {
    return false;
  };
  if let Some(key) = input
    .explicit_model
    .as_ref()
    .or(input.pinned_model.as_ref())
  {
    return !input
      .candidates
      .iter()
      .any(|m| &m.key == key && validation::eligible(m, &request.constraints));
  }
  if input.candidates.is_empty() {
    validation::needs_catalog(&request.constraints)
  } else {
    !input
      .candidates
      .iter()
      .any(|m| validation::eligible(m, &request.constraints))
  }
}
fn finish(mut result: DecisionResult, started: Instant, reason: &str) -> DecisionResult {
  result.elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
  result.provenance.reason_codes.push(reason.into());
  result
}
