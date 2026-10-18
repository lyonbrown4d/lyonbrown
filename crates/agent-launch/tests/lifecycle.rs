mod support;

use agent_launch::{
  model::{ManagedState, ServiceState},
  reconcile::ensure_with,
  state::ServiceLock,
};
use anyhow::Result;
use support::*;

#[tokio::test]
async fn desktop_reuses_native_daemon_installed_by_cli_even_with_older_bundle() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let old = registration("2.0.0");
  lock.write(&ManagedState::Ready(old.clone()))?;
  let service = FakeService::new(Some(old));
  let probe = Probe::new(vec![Ok(daemon("2.0.0", 1))]);
  let info = ensure_with(
    &service,
    &probe,
    &lock,
    registration("1.0.0"),
    &requirements(),
  )
  .await?;
  assert_eq!(info.version.to_string(), "2.0.0");
  assert!(service.installed_versions.borrow().is_empty());
  assert_eq!(service.stops.get(), 0);
  Ok(())
}

#[tokio::test]
async fn unknown_native_registration_is_not_overwritten() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let service = FakeService::new(Some(registration("1.0.0")));
  let probe = Probe::new(vec![]);
  let error = ensure_with(
    &service,
    &probe,
    &lock,
    registration("2.0.0"),
    &requirements(),
  )
  .await
  .expect_err("unknown identity");
  assert!(error.to_string().contains("without managed identity"));
  assert!(service.installed_versions.borrow().is_empty());
  Ok(())
}

#[tokio::test]
async fn initial_install_is_registered_once_and_authenticated() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let service = FakeService::new(None);
  let probe = Probe::new(vec![Ok(daemon("1.0.0", 1))]);
  ensure_with(
    &service,
    &probe,
    &lock,
    registration("1.0.0"),
    &requirements(),
  )
  .await?;
  assert_eq!(*service.installed_versions.borrow(), ["1.0.0"]);
  assert!(matches!(lock.read()?, Some(ManagedState::Ready(_))));
  Ok(())
}

#[tokio::test]
async fn incompatibility_never_downgrades_a_newer_service() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let old = registration("2.0.0");
  lock.write(&ManagedState::Ready(old.clone()))?;
  let service = FakeService::new(Some(old));
  let probe = Probe::new(vec![Ok(daemon("2.0.0", 2))]);
  let error = ensure_with(
    &service,
    &probe,
    &lock,
    registration("1.0.0"),
    &requirements(),
  )
  .await
  .expect_err("downgrade");
  assert!(error.to_string().contains("downgrade"));
  assert_eq!(service.stops.get(), 0);
  Ok(())
}

#[tokio::test]
async fn active_tasks_and_schema_changes_both_defer_incompatible_upgrade() -> Result<()> {
  for (active_runs, schema_max) in [(1, 1), (0, 2)] {
    let temp = tempfile::tempdir()?;
    let lock = ServiceLock::acquire(temp.path())?;
    let old = registration("1.0.0");
    lock.write(&ManagedState::Ready(old.clone()))?;
    let service = FakeService::new(Some(old));
    let mut info = daemon("1.0.0", 2);
    info.active_runs = active_runs;
    let mut desired = registration("2.0.0");
    desired.storage_schema_max = schema_max;
    let probe = Probe::new(vec![Ok(info)]);
    assert!(
      ensure_with(&service, &probe, &lock, desired, &requirements())
        .await
        .is_err()
    );
    assert_eq!(service.stops.get(), 0);
    assert_eq!(probe.drains.get(), 0);
  }
  Ok(())
}

#[tokio::test]
async fn failed_native_activation_restores_old_registration_and_version() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let old = registration("1.0.0");
  lock.write(&ManagedState::Ready(old.clone()))?;
  let service = FakeService::new(Some(old.clone()));
  service.fail_new_start.set(true);
  let probe = Probe::new(vec![Ok(daemon("1.0.0", 2)), Ok(daemon("1.0.0", 2))]);
  let error = ensure_with(
    &service,
    &probe,
    &lock,
    registration("2.0.0"),
    &requirements(),
  )
  .await
  .expect_err("activation failed");
  assert!(error.to_string().contains("restored"));
  assert_eq!(*service.installed_versions.borrow(), ["2.0.0", "1.0.0"]);
  assert_eq!(service.registration.borrow().as_ref(), Some(&old));
  assert!(matches!(lock.read()?, Some(ManagedState::Ready(state)) if state == old));
  Ok(())
}

#[tokio::test]
async fn unresponsive_running_daemon_preserves_transition_instead_of_blind_kill() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let service = FakeService::new(None);
  let probe = Probe::new(vec![
    Err(anyhow::anyhow!("disconnected")),
    Err(anyhow::anyhow!("disconnected")),
  ]);
  assert!(
    ensure_with(
      &service,
      &probe,
      &lock,
      registration("1.0.0"),
      &requirements()
    )
    .await
    .is_err()
  );
  assert_eq!(service.stops.get(), 0);
  assert_eq!(service.state.get(), ServiceState::Running);
  assert!(matches!(
    lock.read()?,
    Some(ManagedState::Transition { .. })
  ));
  Ok(())
}

#[test]
fn native_registration_lock_prevents_concurrent_clients() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let first = ServiceLock::acquire(temp.path())?;
  assert!(ServiceLock::acquire(temp.path()).is_err());
  drop(first);
  assert!(ServiceLock::acquire(temp.path()).is_ok());
  Ok(())
}

#[tokio::test]
async fn shared_registration_rejects_another_data_directory_without_replacing_service() -> Result<()>
{
  let registry = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(registry.path())?;
  let old = registration("1.0.0");
  lock.write(&ManagedState::Ready(old.clone()))?;
  let service = FakeService::new(Some(old.clone()));
  let mut desired = registration("2.0.0");
  desired.data_dir = registry.path().join("other-client-data");
  let probe = Probe::new(vec![]);
  let error = ensure_with(&service, &probe, &lock, desired, &requirements())
    .await
    .expect_err("one user cannot register another daemon with a different data directory");
  assert!(error.to_string().contains("different data directory"));
  assert_eq!(service.stops.get(), 0);
  assert_eq!(probe.drains.get(), 0);
  assert!(service.installed_versions.borrow().is_empty());
  assert!(matches!(lock.read()?, Some(ManagedState::Ready(value)) if value == old));
  Ok(())
}

#[tokio::test]
async fn task_arriving_during_maintenance_request_defers_upgrade_without_stopping() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let old = registration("1.0.0");
  lock.write(&ManagedState::Ready(old.clone()))?;
  let service = FakeService::new(Some(old.clone()));
  let probe = Probe::new(vec![Ok(daemon("1.0.0", 2))]);
  probe.drain_ready.set(false);
  assert!(
    ensure_with(
      &service,
      &probe,
      &lock,
      registration("2.0.0"),
      &requirements()
    )
    .await
    .is_err()
  );
  assert_eq!(service.stops.get(), 0);
  assert!(matches!(lock.read()?, Some(ManagedState::Ready(value)) if value == old));
  Ok(())
}

#[tokio::test]
async fn lost_maintenance_reply_preserves_intent_and_does_not_replay() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let old = registration("1.0.0");
  lock.write(&ManagedState::Ready(old.clone()))?;
  let service = FakeService::new(Some(old));
  let probe = Probe::new(vec![Ok(daemon("1.0.0", 2))]);
  probe.fail_drain.set(true);
  assert!(
    ensure_with(
      &service,
      &probe,
      &lock,
      registration("2.0.0"),
      &requirements()
    )
    .await
    .is_err()
  );
  assert_eq!(service.stops.get(), 0);
  assert!(matches!(
    lock.read()?,
    Some(ManagedState::Transition { .. })
  ));
  assert!(
    ensure_with(
      &service,
      &probe,
      &lock,
      registration("2.0.0"),
      &requirements()
    )
    .await
    .is_err()
  );
  assert_eq!(probe.drains.get(), 1);
  Ok(())
}
