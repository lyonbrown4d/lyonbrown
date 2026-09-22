use std::collections::HashSet;

use agent_core::{ContextItem, ContextItemKind};

use crate::TokenEstimator;

#[derive(Debug, Clone)]
pub struct BudgetResult {
    pub items: Vec<ContextItem>,
    pub tokens: u32,
    pub dropped: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    estimator: TokenEstimator,
}

impl ContextBudget {
    pub fn new(estimator: TokenEstimator) -> Self {
        Self { estimator }
    }

    pub fn trim(&self, items: &[ContextItem], available_tokens: u32) -> BudgetResult {
        let protected = protected_indices(items);
        let mut ranked: Vec<_> = items.iter().enumerate().collect();
        ranked.sort_by_key(|(index, item)| (item.priority, item.timestamp, *index));

        let mut selected: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|(index, _)| protected.contains(index))
            .map(|(index, item)| (index, item.clone()))
            .collect();
        let mut tokens = selected.iter().fold(0_u32, |total, (_, item)| {
            total.saturating_add(self.estimator.estimate(&item.content))
        });
        for (index, item) in ranked {
            if protected.contains(&index) {
                continue;
            }
            let estimate = self.estimator.estimate(&item.content);
            if tokens.saturating_add(estimate) <= available_tokens {
                tokens = tokens.saturating_add(estimate);
                selected.push((index, item.clone()));
            }
        }
        selected.sort_by_key(|(index, item)| (item.priority, item.timestamp, *index));
        let dropped = items.len().saturating_sub(selected.len());
        BudgetResult {
            items: selected.into_iter().map(|(_, item)| item).collect(),
            tokens,
            dropped,
        }
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
        .max_by_key(|(index, item)| (item.timestamp, *index))
    {
        protected.insert(index);
    }
    protected
}
