use crate::{validation, *};

pub(crate) fn domain<'a>(text: &str, policy: &'a Policy) -> Option<&'a DomainRule> {
  let normalized = text.to_lowercase();
  let mut found = policy.domains.iter().filter(|domain| {
    domain
      .terms
      .iter()
      .any(|term| matches_term(&normalized, &term.to_lowercase()))
  });
  let first = found.next()?;
  if found.next().is_some() {
    None
  } else {
    Some(first)
  }
}
fn matches_term(text: &str, term: &str) -> bool {
  if term.is_ascii() {
    text.match_indices(term).any(|(offset, _)| {
      let word_character = |c: char| c.is_alphanumeric() || c == '_';
      !text[..offset]
        .chars()
        .next_back()
        .is_some_and(word_character)
        && !text[offset + term.len()..]
          .chars()
          .next()
          .is_some_and(word_character)
    })
  } else {
    text.contains(term)
  }
}

pub(crate) fn evaluate(request: &DecisionRequest, policy: &Policy) -> Option<DecisionOutput> {
  match &request.operation {
    Operation::RouteModel(input) => {
      let fixed = input
        .explicit_model
        .as_ref()
        .or(input.pinned_model.as_ref());
      if let Some(key) = fixed {
        let model = input
          .candidates
          .iter()
          .find(|m| &m.key == key && validation::eligible(m, &request.constraints))?;
        return Some(DecisionOutput::RouteModel(RouteAdvice {
          complexity: model.tier.into(),
          task_domain: domain(&input.text, policy)
            .map_or("unknown", |d| &d.label)
            .into(),
          recommended_tier: model.tier,
          required_capabilities: request.constraints.required_capabilities.clone(),
          candidate_model_keys: vec![model.key.clone()],
        }));
      }
      let matched = domain(&input.text, policy)?;
      route(request, policy, &matched.label, matched.tier)
    }
    Operation::Classify(input) => {
      let matched = domain(&input.text, policy)?;
      input
        .labels
        .contains(&matched.label)
        .then(|| DecisionOutput::Classify {
          label: matched.label.clone(),
          alternatives: vec![],
        })
    }
    Operation::ContextAdvice(input) => Some(DecisionOutput::ContextAdvice {
      should_compact: validation::compact(input, policy),
      available_tokens: input
        .capacity_tokens
        .saturating_sub(input.reserved_output_tokens)
        .saturating_sub(input.used_tokens),
    }),
    Operation::RecommendTools(input) => {
      let text = input.text.to_lowercase();
      let mut tools: Vec<_> = input
        .authorized_tools
        .iter()
        .filter_map(|tool| {
          let description = tool.description.to_lowercase();
          let terms: Vec<_> = description
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|word| word.chars().count() >= 3)
            .collect();
          let matched = terms
            .iter()
            .filter(|term| matches_term(&text, term))
            .count();
          (matched > 0).then(|| ToolRecommendation {
            id: tool.id.clone(),
            score: matched as f64 / terms.len() as f64,
          })
        })
        .collect();
      tools.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
      tools.truncate(input.top_k);
      (!tools.is_empty()).then_some(DecisionOutput::RecommendTools { tools })
    }
    _ => None,
  }
}

pub(crate) fn route(
  request: &DecisionRequest,
  policy: &Policy,
  domain: &str,
  desired: ModelTier,
) -> Option<DecisionOutput> {
  let Operation::RouteModel(input) = &request.operation else {
    return None;
  };
  let mut candidates: Vec<_> = input
    .candidates
    .iter()
    .filter(|m| validation::eligible(m, &request.constraints))
    .collect();
  candidates.sort_by_key(|m| {
    (
      m.tier.abs_diff(desired),
      policy.local_first && !m.local,
      m.estimated_cost_microusd,
      &m.key,
    )
  });
  let tier = if let Some(model) = candidates.first() {
    model.tier
  } else if input.candidates.is_empty() && !validation::needs_catalog(&request.constraints) {
    if request.constraints.allowed_tiers.is_empty()
      || request.constraints.allowed_tiers.contains(&desired)
    {
      desired
    } else {
      *request
        .constraints
        .allowed_tiers
        .iter()
        .min_by_key(|t| t.abs_diff(desired))?
    }
  } else {
    return None;
  };
  Some(DecisionOutput::RouteModel(RouteAdvice {
    complexity: desired.into(),
    task_domain: domain.into(),
    recommended_tier: tier,
    required_capabilities: request.constraints.required_capabilities.clone(),
    candidate_model_keys: candidates
      .into_iter()
      .filter(|m| m.tier == tier)
      .map(|m| m.key.clone())
      .collect(),
  }))
}
impl ModelTier {
  fn abs_diff(self, other: Self) -> u8 {
    (self as u8).abs_diff(other as u8)
  }
}
