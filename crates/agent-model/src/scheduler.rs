use agent_core::{DataClassification, Locality, ModelCapabilities, ModelTier};

use crate::{ModelProvider, ModelRegistry, RegistryError, SelectedModel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePhase {
    Pre,
    Post,
}

#[derive(Debug, Clone)]
pub struct RoutingPolicy {
    pub prefer_local: bool,
    pub remote_escalation: bool,
    pub max_remote_cost_usd: f64,
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        Self {
            prefer_local: true,
            remote_escalation: true,
            max_remote_cost_usd: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StepContext {
    pub prompt: String,
    pub needs_tools: bool,
    pub needs_json: bool,
    pub previous_steps: usize,
    pub escalated: bool,
    pub classification: DataClassification,
    pub provider: Option<String>,
    pub model: Option<String>,
}

impl StepContext {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            needs_tools: false,
            needs_json: false,
            previous_steps: 0,
            escalated: false,
            classification: DataClassification::Public,
            provider: None,
            model: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerDecision {
    pub provider: String,
    pub model: String,
    pub phase: RoutePhase,
    pub reason: String,
}

pub struct SchedulerSelection {
    pub provider: std::sync::Arc<dyn ModelProvider>,
    pub capabilities: ModelCapabilities,
    pub decision: SchedulerDecision,
    pub complexity: Complexity,
}

pub struct ModelScheduler {
    registry: ModelRegistry,
    policy: RoutingPolicy,
}

impl ModelScheduler {
    pub fn new(registry: ModelRegistry, policy: RoutingPolicy) -> Self {
        Self { registry, policy }
    }

    pub async fn decide(
        &self,
        context: &StepContext,
    ) -> Result<SchedulerSelection, SchedulerError> {
        let complexity = estimate_complexity(context);
        let mut candidates = self.registry.available_models().await?;
        candidates.retain(|candidate| self.is_allowed(candidate, context));
        if candidates.is_empty() {
            let requirement = if context.needs_tools {
                "tool-capable model"
            } else if context.needs_json {
                "JSON-capable model"
            } else {
                "model matching routing constraints"
            };
            return Err(SchedulerError::NoMatch(requirement.into()));
        }

        let desired_tier = if context.escalated && self.policy.remote_escalation {
            ModelTier::Frontier
        } else if context.needs_tools || complexity != Complexity::Low {
            ModelTier::Worker
        } else {
            ModelTier::Control
        };
        candidates.sort_by(|left, right| {
            rank(left, desired_tier, self.policy.prefer_local).total_cmp(&rank(
                right,
                desired_tier,
                self.policy.prefer_local,
            ))
        });
        let SelectedModel {
            provider,
            capabilities,
        } = candidates.remove(0);
        let reason = format!(
            "{:?} complexity; selected {:?}{}",
            complexity,
            capabilities.tier,
            if context.needs_tools {
                " with tools capability"
            } else {
                ""
            }
        );
        Ok(SchedulerSelection {
            decision: SchedulerDecision {
                provider: capabilities.provider.clone(),
                model: capabilities.model.clone(),
                phase: if context.escalated {
                    RoutePhase::Post
                } else {
                    RoutePhase::Pre
                },
                reason,
            },
            provider,
            capabilities,
            complexity,
        })
    }

    fn is_allowed(&self, candidate: &SelectedModel, context: &StepContext) -> bool {
        let capabilities = &candidate.capabilities;
        if context.needs_tools && !capabilities.supports_tools
            || context.needs_json && !capabilities.supports_json
        {
            return false;
        }
        if matches!(
            context.classification,
            DataClassification::Confidential | DataClassification::Secret
        ) && self.policy.prefer_local
            && capabilities.locality != Locality::Local
        {
            return false;
        }
        if let Some(provider) = &context.provider
            && provider != &capabilities.provider
        {
            return false;
        }
        if let Some(model) = &context.model
            && model != &capabilities.model
        {
            return false;
        }
        capabilities.locality == Locality::Local
            || estimated_remote_cost(capabilities, context) <= self.policy.max_remote_cost_usd
    }
}

fn estimated_remote_cost(capabilities: &ModelCapabilities, context: &StepContext) -> f64 {
    let estimated_input_tokens = (context.prompt.len().max(1) as f64) / 4.0;
    let estimated_output_tokens = f64::from(capabilities.max_output_tokens.min(1_024));
    (estimated_input_tokens * capabilities.cost_per_mtok_in
        + estimated_output_tokens * capabilities.cost_per_mtok_out)
        / 1_000_000.0
}

fn rank(candidate: &SelectedModel, desired_tier: ModelTier, prefer_local: bool) -> f64 {
    let tier_penalty = if candidate.capabilities.tier == desired_tier {
        0.0
    } else {
        1_000.0
    };
    let locality_penalty = if prefer_local && candidate.capabilities.locality == Locality::Remote {
        100.0
    } else {
        0.0
    };
    tier_penalty
        + locality_penalty
        + candidate.capabilities.cost_per_mtok_in
        + candidate.capabilities.cost_per_mtok_out
}

fn estimate_complexity(context: &StepContext) -> Complexity {
    let prompt = context.prompt.to_ascii_lowercase();
    let has_code = prompt.contains("```")
        || prompt.contains("fn ")
        || prompt.contains("function ")
        || prompt.contains("class ")
        || prompt.contains("compile")
        || prompt.contains("debug")
        || prompt.contains("refactor");
    if has_code || context.previous_steps >= 2 || prompt.len() > 2_000 {
        Complexity::High
    } else if context.previous_steps > 0 || prompt.len() > 500 {
        Complexity::Medium
    } else {
        Complexity::Low
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error("no {0} is available")]
    NoMatch(String),
}
