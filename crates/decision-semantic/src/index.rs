use crate::{
  EmbeddingDescriptor, EmbeddingProvider,
  embedding::{normalize, similarity},
};
use decision_core::{BackendError, WeightedLabel};
use serde::{Deserialize, Serialize};
use std::{
  collections::{BTreeMap, BTreeSet},
  sync::Arc,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SemanticExample {
  pub label: String,
  pub text: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NegativeExample {
  pub label: String,
  pub text: String,
}

/// Thresholds apply to cosine similarity, which is not a calibrated probability.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SemanticConfig {
  pub index_revision: String,
  pub config_revision: String,
  pub minimum_score: f64,
  pub minimum_margin: f64,
  pub negative_penalty: f64,
  pub max_concurrent_inference: usize,
  pub max_input_bytes: usize,
  pub examples: Vec<SemanticExample>,
  pub hard_negatives: Vec<NegativeExample>,
}
impl SemanticConfig {
  pub(crate) fn validate(&self, descriptor: &EmbeddingDescriptor) -> Result<(), BackendError> {
    let valid_threshold = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
    let labels: BTreeSet<_> = self.examples.iter().map(|e| &e.label).collect();
    if self.index_revision.trim().is_empty()
      || self.index_revision.len() > 128
      || self.config_revision.trim().is_empty()
      || descriptor.model_revision.trim().is_empty()
      || descriptor.preprocessing_revision.trim().is_empty()
      || descriptor.dimensions == 0
      || descriptor.dimensions > 65_536
      || self.examples.is_empty()
      || self.examples.len() > 65_536
      || self.hard_negatives.len() > 65_536
      || self.max_concurrent_inference == 0
      || self.max_concurrent_inference > 1_024
      || self.max_input_bytes == 0
      || !valid_threshold(self.minimum_score)
      || !valid_threshold(self.minimum_margin)
      || !valid_threshold(self.negative_penalty)
      || self.examples.iter().any(|e| {
        e.label.trim().is_empty() || e.text.trim().is_empty() || e.text.len() > self.max_input_bytes
      })
      || self.hard_negatives.iter().any(|e| {
        !labels.contains(&e.label)
          || e.text.trim().is_empty()
          || e.text.len() > self.max_input_bytes
      })
    {
      return Err(BackendError::InvalidInput);
    }
    Ok(())
  }
}

pub(crate) struct Index {
  pub config: SemanticConfig,
  pub descriptor: EmbeddingDescriptor,
  pub provider: Arc<dyn EmbeddingProvider>,
  positives: Vec<(String, Vec<f32>)>,
  negatives: Vec<(String, Vec<f32>)>,
}
impl Index {
  pub fn load(
    config: SemanticConfig,
    provider: Arc<dyn EmbeddingProvider>,
  ) -> Result<Self, BackendError> {
    let descriptor = provider.descriptor();
    config.validate(&descriptor)?;
    let embed = |text: &str| normalize(provider.embed(text)?, descriptor.dimensions);
    let positives = config
      .examples
      .iter()
      .map(|e| Ok((e.label.clone(), embed(&e.text)?)))
      .collect::<Result<_, BackendError>>()?;
    let negatives = config
      .hard_negatives
      .iter()
      .map(|e| Ok((e.label.clone(), embed(&e.text)?)))
      .collect::<Result<_, BackendError>>()?;
    Ok(Self {
      config,
      descriptor,
      provider,
      positives,
      negatives,
    })
  }
  pub fn embed(&self, text: &str) -> Result<Vec<f32>, BackendError> {
    if text.trim().is_empty() || text.len() > self.config.max_input_bytes {
      return Err(BackendError::InvalidInput);
    }
    normalize(self.provider.embed(text)?, self.descriptor.dimensions)
  }
  pub fn rank(&self, query: &[f32], allowed: &BTreeSet<String>) -> Vec<WeightedLabel> {
    let mut scores = BTreeMap::<String, f64>::new();
    for (label, vector) in &self.positives {
      if allowed.contains(label) {
        let score = similarity(query, vector);
        scores
          .entry(label.clone())
          .and_modify(|old| *old = old.max(score))
          .or_insert(score);
      }
    }
    for (label, vector) in &self.negatives {
      if let Some(score) = scores.get_mut(label) {
        *score -= self.config.negative_penalty * similarity(query, vector).max(0.0);
      }
    }
    let mut ranked: Vec<_> = scores
      .into_iter()
      .map(|(label, score)| WeightedLabel {
        label,
        score: score.clamp(0.0, 1.0),
      })
      .collect();
    ranked.sort_by(|a, b| {
      b.score
        .total_cmp(&a.score)
        .then_with(|| a.label.cmp(&b.label))
    });
    ranked
  }
  pub fn accepted(&self, ranked: &[WeightedLabel], minimum: f64) -> bool {
    ranked.first().is_some_and(|top| {
      top.score >= self.config.minimum_score.max(minimum)
        && ranked
          .get(1)
          .is_none_or(|second| top.score - second.score >= self.config.minimum_margin)
    })
  }
}
