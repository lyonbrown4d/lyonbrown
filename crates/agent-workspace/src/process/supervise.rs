use super::{
  CommandOutcome, CommandRequest, CommandStatus, CommandStopReason, platform::ProcessGuard,
};
use std::{io, process::ExitStatus, time::Duration};
use tokio::{
  io::{AsyncRead, AsyncReadExt},
  time::{Instant, sleep_until, timeout},
};
use tokio_util::sync::CancellationToken;

async fn read_chunk<R: AsyncRead + Unpin>(
  pipe: &mut Option<R>,
  remaining: usize,
) -> io::Result<Vec<u8>> {
  let mut bytes = vec![0; (remaining + 1).min(8192)];
  match pipe.as_mut() {
    Some(pipe) => {
      let count = pipe.read(&mut bytes).await?;
      bytes.truncate(count);
      Ok(bytes)
    }
    None => std::future::pending().await,
  }
}

enum Event {
  Exit(io::Result<ExitStatus>),
  Stdout(io::Result<Vec<u8>>),
  Stderr(io::Result<Vec<u8>>),
  Stop(CommandStopReason),
}

pub(super) async fn run(
  mut process: ProcessGuard,
  request: &CommandRequest,
  cancel: CancellationToken,
  deadline: Instant,
) -> CommandOutcome {
  let mut stdout = process.child.stdout().take();
  let mut stderr = process.child.stderr().take();
  let mut out = Vec::new();
  let mut err = Vec::new();
  let mut status = None;
  let mut reason = None;
  let mut drain_deadline = deadline;
  loop {
    if status.is_some() && stdout.is_none() && stderr.is_none() {
      break;
    }
    let remaining = request.output_limit_bytes - out.len() - err.len();
    let event = tokio::select! {
      biased;
      _ = cancel.cancelled() => Event::Stop(CommandStopReason::Cancelled),
      _ = sleep_until(drain_deadline) => Event::Stop(if status.is_some() { CommandStopReason::ProcessIo } else { CommandStopReason::Deadline }),
      // Only one platform child wrapper is installed. Waiting on its direct inner child
      // observes the leader, while the outer group/job remains owned for tree cleanup.
      result = process.child.inner_mut().wait(), if status.is_none() => Event::Exit(result),
      result = read_chunk(&mut stdout, remaining) => Event::Stdout(result),
      result = read_chunk(&mut stderr, remaining) => Event::Stderr(result),
    };
    match event {
      Event::Exit(Ok(exit)) => {
        status = Some(exit);
        // A successful leader must not leave background descendants alive.
        if process.stop_tree().is_err() || !wait_tree(&mut process).await {
          reason = Some(CommandStopReason::CleanupFailed);
          break;
        }
        drain_deadline = deadline.min(Instant::now() + Duration::from_secs(1));
      }
      Event::Exit(Err(_)) => {
        reason = Some(CommandStopReason::ProcessIo);
        break;
      }
      Event::Stop(stop) => {
        reason = Some(stop);
        break;
      }
      Event::Stdout(result) => {
        if capture(result, &mut stdout, &mut out, remaining, &mut reason) {
          break;
        }
      }
      Event::Stderr(result) => {
        if capture(result, &mut stderr, &mut err, remaining, &mut reason) {
          break;
        }
      }
    }
  }
  let output_truncated = reason == Some(CommandStopReason::OutputLimit);
  if reason.is_some() {
    if process.stop_tree().is_err() {
      reason = Some(CommandStopReason::CleanupFailed);
    }
    if status.is_none() {
      match timeout(Duration::from_secs(1), process.child.wait()).await {
        Ok(Ok(exit)) => status = Some(exit),
        _ => reason = Some(CommandStopReason::CleanupFailed),
      }
    }
  }
  CommandOutcome {
    action_id: request.action_id.clone(),
    status: if reason.is_some() {
      CommandStatus::Uncertain
    } else if status.is_some_and(|s| s.success()) {
      CommandStatus::Succeeded
    } else {
      CommandStatus::Failed
    },
    output_truncated,
    reason,
    exit_code: status.and_then(|s| s.code()),
    stdout: String::from_utf8_lossy(&out).into_owned(),
    stderr: String::from_utf8_lossy(&err).into_owned(),
  }
}

async fn wait_tree(process: &mut ProcessGuard) -> bool {
  matches!(
    timeout(Duration::from_secs(1), process.child.wait()).await,
    Ok(Ok(_))
  )
}

fn capture<R>(
  result: io::Result<Vec<u8>>,
  pipe: &mut Option<R>,
  destination: &mut Vec<u8>,
  remaining: usize,
  reason: &mut Option<CommandStopReason>,
) -> bool {
  match result {
    Ok(bytes) => {
      if bytes.is_empty() {
        *pipe = None;
      }
      destination.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
      if bytes.len() > remaining {
        *reason = Some(CommandStopReason::OutputLimit);
        return true;
      }
      false
    }
    Err(_) => {
      *reason = Some(CommandStopReason::ProcessIo);
      true
    }
  }
}
