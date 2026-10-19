use crate::*;
use std::collections::BTreeSet;

pub(crate) fn request_valid(request: &DecisionRequest, policy: &Policy) -> bool {
  if request.request_id.is_empty()
    || request.request_id.len() > 128
    || request
      .policy_revision
      .as_ref()
      .is_some_and(|v| v.len() > 128)
    || request.deadline_ms == 0
    || !within_json_limit(request, policy.max_input_bytes)
  {
    return false;
  }
  let valid_names = |names: Vec<&str>| {
    !names.is_empty()
      && names.len() <= policy.max_candidates
      && names.iter().all(|n| !n.trim().is_empty())
      && names.iter().copied().collect::<BTreeSet<_>>().len() == names.len()
  };
  match &request.operation {
    Operation::RouteModel(input) => {
      input.candidates.len() <= policy.max_candidates
        && (input.candidates.is_empty()
          || valid_names(input.candidates.iter().map(|m| m.key.as_str()).collect()))
        && input.explicit_model.as_ref().is_none_or(|v| !v.is_empty())
        && input.pinned_model.as_ref().is_none_or(|v| !v.is_empty())
    }
    Operation::Classify(input) => valid_names(input.labels.iter().map(String::as_str).collect()),
    Operation::Choice(input) => valid_names(input.options.iter().map(String::as_str).collect()),
    Operation::Score(input) => {
      input.minimum.is_finite() && input.maximum.is_finite() && input.minimum < input.maximum
    }
    Operation::RecommendTools(input) => {
      input.top_k > 0
        && input.top_k <= policy.max_candidates
        && input.authorized_tools.len() <= policy.max_candidates
        && (input.authorized_tools.is_empty()
          || valid_names(
            input
              .authorized_tools
              .iter()
              .map(|m| m.id.as_str())
              .collect(),
          ))
    }
    Operation::ContextAdvice(input) => {
      input.capacity_tokens > 0 && input.reserved_output_tokens <= input.capacity_tokens
    }
    Operation::Predicate(_) => true,
  }
}

pub(crate) fn eligible(model: &ModelCandidate, constraints: &Constraints) -> bool {
  (!constraints.local_only || model.local)
    && (constraints.allowed_tiers.is_empty() || constraints.allowed_tiers.contains(&model.tier))
    && constraints
      .allowed_model_keys
      .as_ref()
      .is_none_or(|keys| keys.contains(&model.key))
    && constraints
      .required_capabilities
      .is_subset(&model.capabilities)
    && constraints.input_tokens.is_none_or(|tokens| {
      model
        .context_tokens
        .is_some_and(|capacity| capacity >= tokens)
    })
    && constraints.max_estimated_cost_microusd.is_none_or(|limit| {
      model
        .estimated_cost_microusd
        .is_some_and(|cost| cost <= limit)
    })
}

pub(crate) fn route_constraints(request: &DecisionRequest, advice: &RouteAdvice) -> Constraints {
  let mut constraints = request.constraints.clone();
  constraints
    .required_capabilities
    .extend(advice.required_capabilities.iter().cloned());
  constraints
}

pub(crate) fn needs_catalog(constraints: &Constraints) -> bool {
  constraints.local_only
    || constraints.allowed_model_keys.is_some()
    || !constraints.required_capabilities.is_empty()
    || constraints.input_tokens.is_some()
    || constraints.max_estimated_cost_microusd.is_some()
}

/// Revalidate every backend result against caller-owned candidates and operation bounds.
pub(crate) fn output_valid(
  request: &DecisionRequest,
  output: &DecisionOutput,
  policy: &Policy,
) -> bool {
  if !within_json_limit(output, policy.max_output_bytes) {
    return false;
  }
  let labels_valid = |values: &[WeightedLabel], allowed: &[String]| {
    values.len() <= policy.max_candidates
      && values
        .iter()
        .all(|v| allowed.contains(&v.label) && probability(v.score))
      && values
        .iter()
        .map(|v| &v.label)
        .collect::<BTreeSet<_>>()
        .len()
        == values.len()
  };
  match (&request.operation, output) {
    (Operation::RouteModel(input), DecisionOutput::RouteModel(advice)) => {
      let constraints = route_constraints(request, advice);
      let keys = &advice.candidate_model_keys;
      (advice.task_domain == "unknown"
        || policy.domains.iter().any(|d| d.label == advice.task_domain))
        && (constraints.allowed_tiers.is_empty()
          || constraints.allowed_tiers.contains(&advice.recommended_tier))
        && request
          .constraints
          .required_capabilities
          .is_subset(&advice.required_capabilities)
        && keys.len() <= policy.max_candidates
        && keys.iter().collect::<BTreeSet<_>>().len() == keys.len()
        && keys.iter().all(|key| {
          input.candidates.iter().any(|m| {
            &m.key == key && eligible(m, &constraints) && m.tier == advice.recommended_tier
          })
        })
        && (if input.candidates.is_empty() {
          keys.is_empty() && !needs_catalog(&constraints)
        } else {
          !keys.is_empty()
        })
        && input
          .explicit_model
          .as_ref()
          .or(input.pinned_model.as_ref())
          .is_none_or(|key| keys.len() == 1 && keys.first() == Some(key))
    }
    (
      Operation::Classify(input),
      DecisionOutput::Classify {
        label,
        alternatives,
      },
    ) => input.labels.contains(label) && labels_valid(alternatives, &input.labels),
    (Operation::Predicate(_), DecisionOutput::Predicate { confidence, .. }) => {
      probability(*confidence) && *confidence >= policy.minimum_confidence
    }
    (
      Operation::Choice(input),
      DecisionOutput::Choice {
        selected,
        distribution,
      },
    ) => {
      input.options.contains(selected)
        && labels_valid(distribution, &input.options)
        && (distribution.is_empty()
          || (distribution.iter().any(|d| &d.label == selected)
            && (distribution.iter().map(|v| v.score).sum::<f64>() - 1.0).abs() <= 0.000_001))
    }
    (Operation::Score(input), DecisionOutput::Score { value }) => {
      value.is_finite() && *value >= input.minimum && *value <= input.maximum
    }
    (Operation::RecommendTools(input), DecisionOutput::RecommendTools { tools }) => {
      tools.len() <= input.top_k
        && tools.iter().all(|v| {
          probability(v.score) && input.authorized_tools.iter().any(|tool| tool.id == v.id)
        })
        && tools.iter().map(|v| &v.id).collect::<BTreeSet<_>>().len() == tools.len()
    }
    (
      Operation::ContextAdvice(input),
      DecisionOutput::ContextAdvice {
        should_compact,
        available_tokens,
      },
    ) => {
      let available = input
        .capacity_tokens
        .saturating_sub(input.reserved_output_tokens)
        .saturating_sub(input.used_tokens);
      *available_tokens == available && *should_compact == compact(input, policy)
    }
    _ => false,
  }
}
pub(crate) fn probability(value: f64) -> bool {
  value.is_finite() && (0.0..=1.0).contains(&value)
}
pub(crate) fn compact(input: &ContextInput, policy: &Policy) -> bool {
  u128::from(
    input
      .used_tokens
      .saturating_add(input.reserved_output_tokens),
  ) * 100
    >= u128::from(input.capacity_tokens) * u128::from(policy.compact_threshold_percent)
}

// Serialize into a counting sink so oversized caller/backend values do not allocate a second full buffer.
fn within_json_limit(value: &impl serde::Serialize, limit: usize) -> bool {
  struct LimitedWriter {
    remaining: usize,
  }
  impl std::io::Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
      if bytes.len() > self.remaining {
        return Err(std::io::Error::other("decision size limit"));
      }
      self.remaining -= bytes.len();
      Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
      Ok(())
    }
  }
  serde_json::to_writer(LimitedWriter { remaining: limit }, value).is_ok()
}

pub(crate) fn backend_metadata_valid(
  decision: &BackendDecision,
  descriptor: &BackendDescriptor,
) -> bool {
  probability(decision.confidence)
    && decision.reason_codes.len() <= 16
    && decision.reason_codes.iter().all(|code| {
      !code.is_empty()
        && code.len() <= 64
        && code
          .bytes()
          .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    })
    && !descriptor.name.is_empty()
    && descriptor.name.len() <= 128
    && !descriptor.version.is_empty()
    && descriptor.version.len() <= 128
    && descriptor
      .index_revision
      .as_ref()
      .is_none_or(|v| !v.is_empty() && v.len() <= 128)
}
