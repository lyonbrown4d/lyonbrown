use crate::ModelTier;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRule {
  pub label: String,
  pub terms: Vec<String>,
  pub tier: ModelTier,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
  pub schema_version: u32,
  pub id: String,
  pub revision: String,
  pub default_tier: ModelTier,
  pub local_first: bool,
  pub minimum_confidence: f64,
  pub max_input_bytes: usize,
  pub max_output_bytes: usize,
  pub max_candidates: usize,
  pub max_deadline_ms: u64,
  pub compact_threshold_percent: u8,
  pub domains: Vec<DomainRule>,
}
impl Default for Policy {
  fn default() -> Self {
    Self {
      schema_version: 1,
      id: "default".into(),
      revision: "rules-v1".into(),
      default_tier: ModelTier::Balanced,
      local_first: true,
      minimum_confidence: 0.7,
      max_input_bytes: 262_144,
      max_output_bytes: 65_536,
      max_candidates: 256,
      max_deadline_ms: 5_000,
      compact_threshold_percent: 80,
      domains: vec![
        DomainRule {
          label: "coding".into(),
          terms: vec!["rust".into(), "code".into(), "代码".into(), "编译".into()],
          tier: ModelTier::Advanced,
        },
        DomainRule {
          label: "translation".into(),
          terms: vec!["translate".into(), "翻译".into()],
          tier: ModelTier::Fast,
        },
        DomainRule {
          label: "research".into(),
          terms: vec!["research".into(), "调研".into()],
          tier: ModelTier::Advanced,
        },
        DomainRule {
          label: "summary".into(),
          terms: vec!["summarize".into(), "摘要".into(), "总结".into()],
          tier: ModelTier::Fast,
        },
        DomainRule {
          label: "chat".into(),
          terms: vec!["hello".into(), "你好".into()],
          tier: ModelTier::Fast,
        },
      ],
    }
  }
}
#[derive(Debug, Error)]
#[error("invalid policy: {0}")]
pub struct PolicyError(pub &'static str);

/// Validated immutable configuration. Construct a complete replacement before publishing it.
#[derive(Clone, Debug)]
pub struct PolicySnapshot(Policy);
impl PolicySnapshot {
  pub fn new(policy: Policy) -> Result<Self, PolicyError> {
    use std::collections::BTreeSet;
    if policy.schema_version != 1
      || policy.id.is_empty()
      || policy.revision.is_empty()
      || policy.id.len() > 128
      || policy.revision.len() > 128
    {
      return Err(PolicyError("schema, id or revision"));
    }
    if !policy.minimum_confidence.is_finite()
      || !(0.0..=1.0).contains(&policy.minimum_confidence)
      || policy.max_input_bytes > 16_777_216
      || policy.max_output_bytes > 1_048_576
      || policy.max_candidates > 4096
      || policy.max_deadline_ms > 60_000
      || policy.max_input_bytes == 0
      || policy.max_output_bytes == 0
      || policy.max_candidates == 0
      || policy.max_deadline_ms == 0
      || !(1..=100).contains(&policy.compact_threshold_percent)
    {
      return Err(PolicyError("limits or confidence"));
    }
    let mut labels = BTreeSet::new();
    if policy.domains.len() > policy.max_candidates
      || policy.domains.iter().any(|d| {
        d.label.is_empty()
          || d.label == "unknown"
          || d.label.len() > 128
          || !labels.insert(&d.label)
          || d.terms.is_empty()
          || d.terms.len() > policy.max_candidates
          || d
            .terms
            .iter()
            .any(|term| term.trim().is_empty() || term.len() > 4096)
      })
    {
      return Err(PolicyError(
        "domain labels and terms must be nonempty and unique",
      ));
    }
    Ok(Self(policy))
  }
  pub fn policy(&self) -> &Policy {
    &self.0
  }
}
