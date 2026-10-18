use anyhow::{Context, Result, bail, ensure};

use crate::{
  model::{
    DaemonInfo, DaemonProbe, ManagedState, NativeService, Registration, Requirements, ServiceState,
  },
  state::ServiceLock,
};

/// Runs while holding the per-user lock. Unknown service state is never overwritten.
pub async fn ensure_with(
  service: &impl NativeService,
  probe: &impl DaemonProbe,
  lock: &ServiceLock,
  desired: Registration,
  requirements: &Requirements,
) -> Result<DaemonInfo> {
  ensure_with_preparation(service, probe, lock, desired, requirements, Ok).await
}

/// Prepare the retained binary only when activation is necessary. A compatible
/// running SCM service can be reused by a normal client without machine writes.
pub async fn ensure_with_preparation(
  service: &impl NativeService,
  probe: &impl DaemonProbe,
  lock: &ServiceLock,
  desired: Registration,
  requirements: &Requirements,
  prepare: impl FnOnce(Registration) -> Result<Registration>,
) -> Result<DaemonInfo> {
  let status = service.status()?;
  let previous = match lock.read()? {
    Some(ManagedState::Ready(registration)) => Some(registration),
    Some(ManagedState::Transition { .. }) => bail!(
      "unfinished native service transition; inspect service.json and native service status before recovery"
    ),
    None => None,
  };
  if status != ServiceState::Absent {
    let old = previous.as_ref().context(
      "native service is registered without managed identity; refusing duplicate registration",
    )?;
    service.validate(old)?;
    ensure!(
      old.data_dir == desired.data_dir,
      "registered daemon uses a different data directory"
    );
    if status == ServiceState::Stopped {
      service.start()?;
    }
    let daemon = probe
      .inspect()
      .await
      .context("registered agentd failed authenticated handshake; service was not replaced")?;
    ensure!(
      daemon.version == old.version,
      "registered binary version differs from authenticated daemon; refusing replacement"
    );
    ensure!(
      !daemon.draining,
      "registered daemon is in maintenance mode; inspect the pending upgrade before reconnecting"
    );
    if requirements.check(&daemon).is_ok() {
      return Ok(daemon);
    }
    ensure!(
      desired.version > daemon.version,
      "incompatible daemon {} cannot be replaced by bundle {} (downgrade or same-version replacement forbidden)",
      daemon.version,
      desired.version
    );
    ensure!(
      daemon.active_runs == 0,
      "daemon has active tasks; upgrade deferred until they finish"
    );
    ensure!(
      desired.storage_schema_min == daemon.storage_schema_version
        && desired.storage_schema_max == daemon.storage_schema_version,
      "automatic upgrade requires the same storage schema for safe binary rollback"
    );
  } else if let Some(old) = &previous {
    ensure!(
      desired.version >= old.version,
      "bundle would downgrade registered daemon {}",
      old.version
    );
  }

  let desired = prepare(desired)?;
  lock.write(&ManagedState::Transition {
    previous: previous.clone(),
    desired: desired.clone(),
  })?;
  if status != ServiceState::Absent
    && !probe.drain().await.context(
      "daemon maintenance result is unknown; inspect service.json and daemon state before retrying",
    )?
  {
    if let Some(old) = previous {
      lock.write(&ManagedState::Ready(old))?;
    }
    bail!("new active tasks appeared; task acceptance resumed and upgrade deferred");
  }
  let result = replace(service, probe, status, &desired, requirements).await;
  match result {
    Ok(info) => {
      lock.write(&ManagedState::Ready(desired))?;
      Ok(info)
    }
    Err(error) => {
      rollback(service, probe, lock, previous.as_ref())
        .await
        .with_context(|| {
          format!("activation failed ({error:#}); rollback could not finish; inspect service.json")
        })?;
      Err(error.context("activation failed; previous service registration restored"))
    }
  }
}

async fn replace(
  service: &impl NativeService,
  probe: &impl DaemonProbe,
  status: ServiceState,
  desired: &Registration,
  requirements: &Requirements,
) -> Result<DaemonInfo> {
  if status != ServiceState::Absent {
    service.stop()?;
    service.update(desired)?;
  } else {
    service.install(desired)?;
  }
  // Some native managers start as part of installation.
  if service.status()? != ServiceState::Running {
    service.start()?;
  }
  service.validate(desired)?;
  let info = probe.inspect().await?;
  ensure!(
    info.version == desired.version,
    "activated daemon reports a different version"
  );
  requirements.check(&info)?;
  Ok(info)
}

async fn rollback(
  service: &impl NativeService,
  probe: &impl DaemonProbe,
  lock: &ServiceLock,
  previous: Option<&Registration>,
) -> Result<()> {
  let state = service.status()?;
  if state == ServiceState::Running {
    let info = probe
      .inspect()
      .await
      .context("new daemon is running but its task state is unknown; refusing to stop it")?;
    ensure!(
      info.active_runs == 0,
      "new daemon has active tasks; automatic rollback deferred"
    );
    ensure!(
      probe.drain().await?,
      "new daemon became busy; rollback deferred"
    );
    service.stop()?;
  }
  if let Some(old) = previous {
    if service.status()? == ServiceState::Absent {
      service.install(old)?;
    } else {
      service.update(old)?;
    }
    if service.status()? != ServiceState::Running {
      service.start()?;
    }
    service.validate(old)?;
    let restored = probe.inspect().await?;
    ensure!(
      restored.version == old.version,
      "restored service version handshake failed"
    );
    lock.write(&ManagedState::Ready(old.clone()))?;
  } else {
    if service.status()? != ServiceState::Absent {
      service.uninstall()?;
    }
    lock.clear()?;
  }
  Ok(())
}
