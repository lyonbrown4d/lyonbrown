use super::audit::AuditGuard;
use crate::{AuditStatus, sse::SseValidator};
use axum::body::Body;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use std::{
  io,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::{
  sync::mpsc,
  time::{Instant, timeout_at},
};

/// A one-chunk queue keeps the deadline live even when the downstream body is never polled.
pub(super) fn body(
  mut source: impl Stream<Item = Result<Bytes, rig_core::http_client::Error>> + Unpin + Send + 'static,
  first: Bytes,
  mut validator: SseValidator,
  mut audit: AuditGuard,
  deadline: Instant,
  max_response: usize,
) -> Body {
  let completion = audit.body_completion();
  let (send, mut receive) = mpsc::channel::<Bytes>(1);
  let failed = Arc::new(AtomicBool::new(false));
  let terminal = failed.clone();
  tokio::spawn(async move {
    let outcome = timeout_at(deadline, async {
      let mut seen = first.len();
      send.send(first).await.map_err(|_| ())?;
      loop {
        if validator.done {
          return Ok::<_, ()>(());
        }
        let next = tokio::select! {
          biased;
          _ = send.closed() => return Err(()),
          next = source.next() => next,
        };
        let Some(Ok(chunk)) = next else {
          return Err(());
        };
        if chunk.len() > max_response.saturating_sub(seen) || validator.check(&chunk).is_err() {
          return Err(());
        }
        seen += chunk.len();
        send.send(chunk).await.map_err(|_| ())?;
      }
    })
    .await;
    // Cancel network reads before awaiting durable terminal state.
    drop(source);
    let status = if matches!(outcome, Ok(Ok(()))) {
      AuditStatus::Succeeded
    } else {
      AuditStatus::Uncertain
    };
    audit.set_usage(validator.usage());
    if matches!(status, AuditStatus::Uncertain) {
      terminal.store(true, Ordering::Release);
    }
    if audit.finish(status).await.is_err() {
      terminal.store(true, Ordering::Release);
    }
    drop(send);
  });
  let stream = async_stream::try_stream! {
    let _completion = completion;
    while let Some(chunk) = receive.recv().await { yield chunk; }
    if failed.load(Ordering::Acquire) { Err(io::Error::other("Upstream stream did not complete"))?; }
  };
  Body::from_stream(stream.map(|item: Result<Bytes, io::Error>| item))
}
