mod support;

use agent_launch::{
  model::{DaemonInfo, ManagedState, NativeService, Registration, ServiceState},
  reconcile::ensure_with,
  state::ServiceLock,
};
use anyhow::{Result, bail};
use std::cell::Cell;
use support::*;

/// Account credentials belong to SCM and cannot be recovered after deletion.
struct AccountService {
  native: FakeService,
  updates: Cell<usize>,
}
impl NativeService for AccountService {
  fn status(&self) -> Result<ServiceState> {
    self.native.status()
  }
  fn validate(&self, value: &Registration) -> Result<()> {
    self.native.validate(value)
  }
  fn install(&self, _: &Registration) -> Result<()> {
    bail!("existing account must not be recreated")
  }
  fn update(&self, value: &Registration) -> Result<()> {
    assert_eq!(self.status()?, ServiceState::Stopped);
    self.updates.set(self.updates.get() + 1);
    self.native.install(value)
  }
  fn start(&self) -> Result<()> {
    self.native.start()
  }
  fn stop(&self) -> Result<()> {
    self.native.stop()
  }
  fn uninstall(&self) -> Result<()> {
    bail!("deleting SCM registration loses stored credentials")
  }
}

async fn activate(fail: bool) -> Result<(AccountService, Result<DaemonInfo>)> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let old = registration("1.0.0");
  lock.write(&ManagedState::Ready(old.clone()))?;
  let service = AccountService {
    native: FakeService::new(Some(old)),
    updates: Cell::new(0),
  };
  service.native.fail_new_start.set(fail);
  let probe = Probe::new(vec![
    Ok(daemon("1.0.0", 2)),
    Ok(daemon(if fail { "1.0.0" } else { "2.0.0" }, 1)),
  ]);
  let result = ensure_with(
    &service,
    &probe,
    &lock,
    registration("2.0.0"),
    &requirements(),
  )
  .await;
  let expected = if fail { "1.0.0" } else { "2.0.0" };
  assert!(
    matches!(lock.read()?, Some(ManagedState::Ready(value)) if value.version.to_string() == expected)
  );
  Ok((service, result))
}

#[tokio::test]
async fn upgrade_changes_existing_account_service_without_deleting_credentials() -> Result<()> {
  let (service, result) = activate(false).await?;
  assert_eq!(result?.version.to_string(), "2.0.0");
  assert_eq!(service.updates.get(), 1);
  Ok(())
}

#[tokio::test]
async fn failed_upgrade_restores_old_configuration_without_recreating_account() -> Result<()> {
  let (service, result) = activate(true).await?;
  assert!(result.unwrap_err().to_string().contains("restored"));
  assert_eq!(service.updates.get(), 2);
  Ok(())
}

#[tokio::test]
async fn compatible_service_reuse_does_not_prepare_or_write_machine_binaries() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let lock = ServiceLock::acquire(temp.path())?;
  let old = registration("1.0.0");
  lock.write(&ManagedState::Ready(old.clone()))?;
  let service = FakeService::new(Some(old));
  let probe = Probe::new(vec![Ok(daemon("1.0.0", 1))]);
  let result = agent_launch::reconcile::ensure_with_preparation(
    &service,
    &probe,
    &lock,
    registration("2.0.0"),
    &requirements(),
    |_| bail!("normal client cannot write machine binaries"),
  )
  .await?;
  assert_eq!(result.version.to_string(), "1.0.0");
  assert!(service.installed_versions.borrow().is_empty());
  Ok(())
}
