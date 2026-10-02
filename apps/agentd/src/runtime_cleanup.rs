use std::time::Duration;

use agent_app::{RuntimeActorError, RuntimeShutdownHandle};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::model_setup::PreparedRuntime;

pub const DEFAULT_RUNTIME_STOP_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn dispose_prepared(prepared: PreparedRuntime, stop_timeout: Duration) {
  let shutdown = prepared.runtime.shutdown_handle();
  drop(prepared.controller);
  let joined = tokio::spawn(prepared.runtime.join());
  stop_joined_runtime(shutdown, joined, stop_timeout).await;
}

pub async fn stop_joined_runtime(
  shutdown: RuntimeShutdownHandle,
  mut joined: JoinHandle<Result<(), RuntimeActorError>>,
  stop_timeout: Duration,
) {
  shutdown.request_shutdown();
  if timeout(stop_timeout, &mut joined).await.is_ok() {
    return;
  }
  shutdown.abort();
  if timeout(stop_timeout, &mut joined).await.is_ok() {
    return;
  }
  joined.abort();
  let _ = timeout(stop_timeout, &mut joined).await;
}

pub async fn stop_background_task(mut task: JoinHandle<()>, stop_timeout: Duration) {
  if timeout(stop_timeout, &mut task).await.is_ok() {
    return;
  }
  task.abort();
  let _ = timeout(stop_timeout, &mut task).await;
}
