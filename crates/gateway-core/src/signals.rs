use crate::{AuditStatus, TokenUsage};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub(crate) struct Signals {
  requests: AtomicU64,
  rejected: AtomicU64,
  header_latency_ms: AtomicU64,
  succeeded: AtomicU64,
  failed: AtomicU64,
  uncertain: AtomicU64,
  completion_latency_ms: AtomicU64,
  requests_with_usage: AtomicU64,
  requests_without_usage: AtomicU64,
  input_tokens: AtomicU64,
  output_tokens: AtomicU64,
  cached_input_tokens: AtomicU64,
  reasoning_tokens: AtomicU64,
}
/// Low-cardinality counters for an embedding host's metrics exporter or administration view.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Observations {
  pub http_requests: u64,
  pub http_rejections: u64,
  pub http_header_latency_ms_total: u64,
  pub completions_succeeded: u64,
  pub completions_failed: u64,
  pub completions_uncertain: u64,
  pub completion_latency_ms_total: u64,
  /// Number of successfully completed requests whose provider supplied usage.
  pub requests_with_usage: u64,
  /// Number of successfully completed requests for which usage was unavailable.
  pub requests_without_usage: u64,
  /// Cumulative provider-reported input tokens.
  pub input_tokens: u64,
  /// Cumulative provider-reported output tokens.
  pub output_tokens: u64,
  /// Cumulative provider-reported cached input tokens.
  pub cached_input_tokens: u64,
  /// Cumulative provider-reported reasoning tokens.
  pub reasoning_tokens: u64,
  pub available_capacity: usize,
}
fn saturating_add(counter: &AtomicU64, amount: u64) {
  let mut current = counter.load(Ordering::Relaxed);
  loop {
    match counter.compare_exchange_weak(
      current,
      current.saturating_add(amount),
      Ordering::Relaxed,
      Ordering::Relaxed,
    ) {
      Ok(_) => return,
      Err(actual) => current = actual,
    }
  }
}

impl Signals {
  pub fn response(&self, success: bool, elapsed_ms: u64) {
    self.requests.fetch_add(1, Ordering::Relaxed);
    if !success {
      self.rejected.fetch_add(1, Ordering::Relaxed);
    }
    self
      .header_latency_ms
      .fetch_add(elapsed_ms, Ordering::Relaxed);
  }
  pub fn terminal(&self, status: AuditStatus, elapsed_ms: u64) {
    let counter = match status {
      AuditStatus::Succeeded => &self.succeeded,
      AuditStatus::Uncertain => &self.uncertain,
      AuditStatus::Failed | AuditStatus::Cancelled => &self.failed,
      AuditStatus::Pending => return,
    };
    saturating_add(counter, 1);
    saturating_add(&self.completion_latency_ms, elapsed_ms);
  }
  /// Records usage only after the corresponding terminal audit write succeeds.
  pub(crate) fn usage(&self, usage: Option<TokenUsage>) {
    let Some(usage) = usage else {
      saturating_add(&self.requests_without_usage, 1);
      return;
    };
    saturating_add(&self.requests_with_usage, 1);
    saturating_add(&self.input_tokens, usage.input_tokens);
    saturating_add(&self.output_tokens, usage.output_tokens);
    saturating_add(&self.cached_input_tokens, usage.cached_input_tokens);
    saturating_add(&self.reasoning_tokens, usage.reasoning_tokens);
  }
  pub fn snapshot(&self, available_capacity: usize) -> Observations {
    Observations {
      http_requests: self.requests.load(Ordering::Relaxed),
      http_rejections: self.rejected.load(Ordering::Relaxed),
      http_header_latency_ms_total: self.header_latency_ms.load(Ordering::Relaxed),
      completions_succeeded: self.succeeded.load(Ordering::Relaxed),
      completions_failed: self.failed.load(Ordering::Relaxed),
      completions_uncertain: self.uncertain.load(Ordering::Relaxed),
      completion_latency_ms_total: self.completion_latency_ms.load(Ordering::Relaxed),
      requests_with_usage: self.requests_with_usage.load(Ordering::Relaxed),
      requests_without_usage: self.requests_without_usage.load(Ordering::Relaxed),
      input_tokens: self.input_tokens.load(Ordering::Relaxed),
      output_tokens: self.output_tokens.load(Ordering::Relaxed),
      cached_input_tokens: self.cached_input_tokens.load(Ordering::Relaxed),
      reasoning_tokens: self.reasoning_tokens.load(Ordering::Relaxed),
      available_capacity,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::saturating_add;
  use std::sync::atomic::{AtomicU64, Ordering};

  #[test]
  fn saturating_add_does_not_wrap() {
    let counter = AtomicU64::new(u64::MAX - 1);
    saturating_add(&counter, 10);
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
  }
}
