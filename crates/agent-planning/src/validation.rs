use crate::{PlanDraft, PlanError, Result};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub(crate) fn text(value: &str, maximum: usize) -> Result<()> {
  if value.trim().is_empty() || value.len() > maximum {
    return Err(PlanError::Invalid("empty or oversized text".into()));
  }
  Ok(())
}
pub fn validate_draft(draft: &PlanDraft) -> Result<()> {
  text(&draft.title, 512)?;
  if draft.steps.is_empty() || draft.steps.len() > 128 {
    return Err(PlanError::Invalid("a plan needs 1..128 steps".into()));
  }
  let mut graph = BTreeMap::new();
  for step in &draft.steps {
    text(&step.title, 512)?;
    if step.id.is_nil() || graph.insert(step.id, &step.dependencies).is_some() {
      return Err(PlanError::Invalid("duplicate or nil step UUID".into()));
    }
    if step.acceptance.is_empty() || step.acceptance.len() > 32 {
      return Err(PlanError::Invalid(
        "a step needs 1..32 acceptance criteria".into(),
      ));
    }
    for criterion in &step.acceptance {
      text(criterion, 2048)?;
    }
    let unique = step.dependencies.iter().collect::<BTreeSet<_>>();
    if unique.len() != step.dependencies.len() {
      return Err(PlanError::Invalid("duplicate dependency".into()));
    }
  }
  for (id, dependencies) in &graph {
    if dependencies
      .iter()
      .any(|dependency| dependency == id || !graph.contains_key(dependency))
    {
      return Err(PlanError::Invalid("unknown or self dependency".into()));
    }
  }
  // Bounded Kahn traversal avoids recursion on untrusted model input.
  let mut visited = BTreeSet::<Uuid>::new();
  loop {
    let before = visited.len();
    for (id, dependencies) in &graph {
      if dependencies
        .iter()
        .all(|dependency| visited.contains(dependency))
      {
        visited.insert(*id);
      }
    }
    if visited.len() == graph.len() {
      return Ok(());
    }
    if visited.len() == before {
      return Err(PlanError::Invalid("cyclic step dependencies".into()));
    }
  }
}

pub(crate) fn bounded_plan(plan: &crate::Plan) -> Result<()> {
  let encoded =
    serde_json::to_vec(plan).map_err(|_| PlanError::Invalid("plan cannot be encoded".into()))?;
  if encoded.len() > crate::MAX_PLAN_BYTES {
    return Err(PlanError::Invalid(
      "plan exceeds the 2 MiB aggregate limit".into(),
    ));
  }
  Ok(())
}
