use std::collections::HashSet;

use agent_core::{ContextItem, ContextItemKind, DataClassification};
use thiserror::Error;

use crate::TokenEstimator;

#[derive(Debug, Clone)]
pub struct BudgetResult {
    pub items: Vec<ContextItem>,
    pub tokens: u64,
    pub dropped: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BudgetError {
    #[error("protected context requires {required} tokens but only {budget} are available")]
    ProtectedMinimumExceeds { required: u64, budget: u64 },
    #[error("context token count overflowed u64")]
    TokenCountOverflow,
}

#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    estimator: TokenEstimator,
}

impl ContextBudget {
    pub fn new(estimator: TokenEstimator) -> Self {
        Self { estimator }
    }

    pub fn trim(
        &self,
        items: &[ContextItem],
        available_tokens: u64,
    ) -> Result<BudgetResult, BudgetError> {
        let protected = protected_indices(items);
        let mut ranked: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.classification != DataClassification::Secret)
            .collect();
        ranked.sort_by_key(|(index, item)| (item.priority, item.timestamp, *index));

        let mut selected: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|(index, _)| protected.contains(index))
            .map(|(index, item)| (index, item.clone()))
            .collect();
        let mut tokens = selected.iter().try_fold(0_u64, |total, (_, item)| {
            total
                .checked_add(self.estimator.estimate(&item.content))
                .ok_or(BudgetError::TokenCountOverflow)
        })?;
        if tokens > available_tokens {
            return Err(BudgetError::ProtectedMinimumExceeds {
                required: tokens,
                budget: available_tokens,
            });
        }
        for (index, item) in ranked {
            if protected.contains(&index) {
                continue;
            }
            let estimate = self.estimator.estimate(&item.content);
            if tokens
                .checked_add(estimate)
                .is_some_and(|total| total <= available_tokens)
            {
                tokens += estimate;
                selected.push((index, item.clone()));
            }
        }
        selected.sort_by_key(|(index, item)| (item.priority, item.timestamp, *index));
        let dropped = items.len().saturating_sub(selected.len());
        Ok(BudgetResult {
            items: selected.into_iter().map(|(_, item)| item).collect(),
            tokens,
            dropped,
        })
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self::new(TokenEstimator::default())
    }
}

fn protected_indices(items: &[ContextItem]) -> HashSet<usize> {
    let mut users: Vec<_> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.kind == ContextItemKind::Message && item.source == "user")
        .filter(|(_, item)| item.classification != DataClassification::Secret)
        .collect();
    users.sort_by_key(|(index, item)| (item.timestamp, *index));

    let mut protected: HashSet<_> = users
        .into_iter()
        .rev()
        .take(3)
        .map(|(index, _)| index)
        .collect();
    if let Some((index, _)) = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.kind == ContextItemKind::ToolObservation)
        .filter(|(_, item)| item.classification != DataClassification::Secret)
        .max_by_key(|(index, item)| (item.timestamp, *index))
    {
        protected.insert(index);
    }
    protected
}
