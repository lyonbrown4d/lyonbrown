use super::PreparedRuntime;

pub(super) async fn dispose_prepared(prepared: PreparedRuntime) {
  crate::runtime_cleanup::dispose_prepared(
    prepared,
    crate::runtime_cleanup::DEFAULT_RUNTIME_STOP_TIMEOUT,
  )
  .await;
}
