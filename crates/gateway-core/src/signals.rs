use crate::AuditStatus;
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
  pub available_capacity: usize,
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
    counter.fetch_add(1, Ordering::Relaxed);
    self
      .completion_latency_ms
      .fetch_add(elapsed_ms, Ordering::Relaxed);
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
      available_capacity,
    }
  }
}
