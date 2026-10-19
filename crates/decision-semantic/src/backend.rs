use crate::{EmbeddingProvider, SemanticConfig, embedding::similarity, index::Index};
use async_trait::async_trait;
use decision_core::{
  BackendDecision, BackendDescriptor, BackendError, Complexity, DecisionBackend,
  DecisionCancellation, DecisionOutput, DecisionRequest, ModelTier, Operation, PolicySnapshot,
  RouteAdvice, ToolRecommendation, eligible_model,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};
use tokio::sync::Semaphore;

/// A corpus snapshot. Construct a replacement completely before publishing it to callers.
/// Cancellation stops waiting; the worker owns its permit until inference actually finishes.
#[derive(Clone)]
pub struct SemanticBackend {
  index: Arc<Index>,
  slots: Arc<Semaphore>,
  descriptor: BackendDescriptor,
}
impl SemanticBackend {
  pub async fn load(
    config: SemanticConfig,
    provider: Arc<dyn EmbeddingProvider>,
  ) -> Result<Self, BackendError> {
    config.validate(&provider.descriptor())?;
    let slots = Arc::new(Semaphore::new(config.max_concurrent_inference));
    let (index, descriptor) = tokio::task::spawn_blocking(move || {
      let index = Index::load(config, provider)?;
      let descriptor = describe(&index);
      Ok::<_, BackendError>((index, descriptor))
    })
    .await
    .map_err(|_| BackendError::Unavailable)??;
    Ok(Self {
      index: Arc::new(index),
      slots,
      descriptor,
    })
  }
  fn decide(
    index: &Index,
    request: &DecisionRequest,
    policy: &PolicySnapshot,
  ) -> Result<BackendDecision, BackendError> {
    let (text, allowed): (&str, BTreeSet<String>) = match &request.operation {
      Operation::RouteModel(input) => (
        &input.text,
        policy
          .policy()
          .domains
          .iter()
          .map(|d| d.label.clone())
          .collect(),
      ),
      Operation::Classify(input) => (&input.text, input.labels.iter().cloned().collect()),
      Operation::RecommendTools(input) => {
        if input.top_k == 0
          || input.top_k > policy.policy().max_candidates
          || input.authorized_tools.len() > policy.policy().max_candidates
          || input
            .authorized_tools
            .iter()
            .map(|t| &t.id)
            .collect::<BTreeSet<_>>()
            .len()
            != input.authorized_tools.len()
          || input
            .authorized_tools
            .iter()
            .any(|t| t.id.trim().is_empty())
        {
          return Err(BackendError::InvalidInput);
        }
        let query = index.embed(&input.text)?;
        let mut tools = Vec::new();
        for candidate in &input.authorized_tools {
          let vector = index.embed(&candidate.description)?;
          let score = similarity(&query, &vector).max(0.0);
          if score
            >= index
              .config
              .minimum_score
              .max(policy.policy().minimum_confidence)
          {
            tools.push(ToolRecommendation {
              id: candidate.id.clone(),
              score,
            });
          }
        }
        tools.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        tools.truncate(input.top_k);
        let confidence = tools.first().map_or(0.0, |tool| tool.score);
        return Ok(BackendDecision {
          output: if tools.is_empty() {
            DecisionOutput::Unknown
          } else {
            DecisionOutput::RecommendTools { tools }
          },
          confidence,
          reason_codes: vec!["SEMANTIC_AUTHORIZED_TOOL_RECALL".into()],
        });
      }
      _ => return Err(BackendError::InvalidInput),
    };
    let query = index.embed(text)?;
    let ranked = index.rank(&query, &allowed);
    let confidence = ranked.first().map_or(0.0, |v| v.score);
    if !index.accepted(&ranked, policy.policy().minimum_confidence) {
      let reason = if ranked.is_empty() {
        "SEMANTIC_NO_CANDIDATES"
      } else if confidence
        < index
          .config
          .minimum_score
          .max(policy.policy().minimum_confidence)
      {
        "SEMANTIC_BELOW_THRESHOLD"
      } else {
        "SEMANTIC_AMBIGUOUS"
      };
      return Ok(BackendDecision {
        output: DecisionOutput::Unknown,
        confidence,
        reason_codes: vec![reason.into()],
      });
    }
    let top = ranked.first().ok_or(BackendError::Unavailable)?;
    let output = match &request.operation {
      Operation::Classify(_) => DecisionOutput::Classify {
        label: top.label.clone(),
        alternatives: ranked,
      },
      Operation::RouteModel(input) => {
        let domain = policy
          .policy()
          .domains
          .iter()
          .find(|d| d.label == top.label)
          .ok_or(BackendError::InvalidInput)?;
        let mut candidates: Vec<_> = input
          .candidates
          .iter()
          .filter(|m| m.tier == domain.tier && eligible_model(m, &request.constraints))
          .collect();
        candidates.sort_by(|a, b| {
          (policy.policy().local_first && b.local)
            .cmp(&(policy.policy().local_first && a.local))
            .then_with(|| a.key.cmp(&b.key))
        });
        DecisionOutput::RouteModel(RouteAdvice {
          task_domain: domain.label.clone(),
          recommended_tier: domain.tier,
          complexity: match domain.tier {
            ModelTier::Fast => Complexity::Low,
            ModelTier::Balanced => Complexity::Medium,
            ModelTier::Advanced => Complexity::High,
          },
          required_capabilities: request.constraints.required_capabilities.clone(),
          candidate_model_keys: candidates.into_iter().map(|m| m.key.clone()).collect(),
        })
      }
      _ => return Err(BackendError::InvalidInput),
    };
    Ok(BackendDecision {
      output,
      confidence,
      reason_codes: vec!["SEMANTIC_ACCEPTED".into()],
    })
  }
}
fn describe(index: &Index) -> BackendDescriptor {
  let descriptor = &index.descriptor;
  BackendDescriptor {
    name: "semantic-cosine".into(),
    version: {
      let mut digest = Sha256::new();
      for value in [
        descriptor.model_revision.as_bytes(),
        descriptor.preprocessing_revision.as_bytes(),
        index.config.config_revision.as_bytes(),
      ] {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value);
      }
      digest.update((descriptor.dimensions as u64).to_le_bytes());
      digest.update((index.config.max_input_bytes as u64).to_le_bytes());
      digest.update((index.config.max_concurrent_inference as u64).to_le_bytes());
      digest.update([
        u8::from(descriptor.local),
        u8::from(cfg!(feature = "semantic-local")),
      ]);
      for value in [
        index.config.minimum_score,
        index.config.minimum_margin,
        index.config.negative_penalty,
      ] {
        digest.update(value.to_le_bytes());
      }
      for (kind, label, text) in index
        .config
        .examples
        .iter()
        .map(|e| (0_u8, &e.label, &e.text))
        .chain(
          index
            .config
            .hard_negatives
            .iter()
            .map(|e| (1_u8, &e.label, &e.text)),
        )
      {
        digest.update([kind]);
        for value in [label.as_bytes(), text.as_bytes()] {
          digest.update((value.len() as u64).to_le_bytes());
          digest.update(value);
        }
      }
      format!(
        "{};contract-sha256={}",
        env!("CARGO_PKG_VERSION"),
        hex::encode(digest.finalize())
      )
    },
    index_revision: Some(index.config.index_revision.clone()),
    local: descriptor.local,
  }
}

#[async_trait]
impl DecisionBackend for SemanticBackend {
  fn descriptor(&self) -> BackendDescriptor {
    self.descriptor.clone()
  }
  fn supports(&self, operation: &Operation) -> bool {
    matches!(
      operation,
      Operation::RouteModel(_) | Operation::Classify(_) | Operation::RecommendTools(_)
    )
  }
  async fn evaluate(
    &self,
    request: &DecisionRequest,
    policy: &PolicySnapshot,
    cancellation: &DecisionCancellation,
  ) -> Result<BackendDecision, BackendError> {
    if !self.supports(&request.operation)
      || (request.constraints.local_only && !self.index.descriptor.local)
    {
      return Err(BackendError::InvalidInput);
    }
    let permit = tokio::select! { biased;
      _ = cancellation.cancelled() => return Err(BackendError::Cancelled),
      permit = self.slots.clone().acquire_owned() => permit.map_err(|_| BackendError::Unavailable)?,
    };
    let index = self.index.clone();
    let request = request.clone();
    let policy = policy.clone();
    let cancelled = cancellation.clone();
    let worker = tokio::task::spawn_blocking(move || {
      let _permit = permit;
      if cancelled.is_cancelled() {
        return Err(BackendError::Cancelled);
      }
      Self::decide(&index, &request, &policy)
    });
    tokio::select! { biased;
      _ = cancellation.cancelled() => Err(BackendError::Cancelled),
      result = worker => result.map_err(|_| BackendError::Unavailable)?,
    }
  }
}
