use std::{
  cell::{Cell, RefCell},
  collections::{BTreeSet, VecDeque},
  path::PathBuf,
};

use agent_launch::model::{
  DaemonInfo, DaemonProbe, NativeService, Registration, Requirements, ServiceState,
};
use anyhow::{Result, bail};

pub struct FakeService {
  pub state: Cell<ServiceState>,
  pub registration: RefCell<Option<Registration>>,
  pub installed_versions: RefCell<Vec<String>>,
  pub fail_new_start: Cell<bool>,
  pub stops: Cell<u32>,
}

impl FakeService {
  pub fn new(old: Option<Registration>) -> Self {
    Self {
      state: Cell::new(if old.is_some() {
        ServiceState::Running
      } else {
        ServiceState::Absent
      }),
      registration: RefCell::new(old),
      installed_versions: RefCell::new(Vec::new()),
      fail_new_start: Cell::new(false),
      stops: Cell::new(0),
    }
  }
}

impl NativeService for FakeService {
  fn status(&self) -> Result<ServiceState> {
    Ok(self.state.get())
  }
  fn validate(&self, registration: &Registration) -> Result<()> {
    assert_eq!(self.registration.borrow().as_ref(), Some(registration));
    Ok(())
  }
  fn install(&self, registration: &Registration) -> Result<()> {
    self
      .installed_versions
      .borrow_mut()
      .push(registration.version.to_string());
    self.registration.replace(Some(registration.clone()));
    self.state.set(ServiceState::Stopped);
    Ok(())
  }
  fn start(&self) -> Result<()> {
    if self.fail_new_start.replace(false) {
      bail!("new service failed to start");
    }
    self.state.set(ServiceState::Running);
    Ok(())
  }
  fn stop(&self) -> Result<()> {
    self.stops.set(self.stops.get() + 1);
    self.state.set(ServiceState::Stopped);
    Ok(())
  }
  fn uninstall(&self) -> Result<()> {
    self.registration.replace(None);
    self.state.set(ServiceState::Absent);
    Ok(())
  }
}

pub struct Probe {
  replies: RefCell<VecDeque<Result<DaemonInfo>>>,
  pub drains: Cell<u32>,
  pub drain_ready: Cell<bool>,
  pub fail_drain: Cell<bool>,
}
impl Probe {
  pub fn new(replies: Vec<Result<DaemonInfo>>) -> Self {
    Self {
      replies: RefCell::new(replies.into()),
      drains: Cell::new(0),
      drain_ready: Cell::new(true),
      fail_drain: Cell::new(false),
    }
  }
}
impl DaemonProbe for Probe {
  async fn inspect(&self) -> Result<DaemonInfo> {
    self
      .replies
      .borrow_mut()
      .pop_front()
      .expect("unexpected handshake")
  }
  async fn drain(&self) -> Result<bool> {
    self.drains.set(self.drains.get() + 1);
    if self.fail_drain.get() {
      bail!("maintenance response lost");
    }
    Ok(self.drain_ready.get())
  }
}

pub fn registration(version: &str) -> Registration {
  Registration {
    executable: PathBuf::from(format!("/daemon/{version}/agentd")),
    data_dir: PathBuf::from("/user/data"),
    version: version.parse().expect("test version"),
    storage_schema_min: 1,
    storage_schema_max: 1,
  }
}
pub fn daemon(version: &str, protocol: u32) -> DaemonInfo {
  DaemonInfo {
    version: version.parse().expect("test version"),
    protocol_min: protocol,
    protocol_max: protocol,
    capabilities: BTreeSet::from(["runs".into()]),
    active_runs: 0,
    storage_schema_version: 1,
    draining: false,
  }
}
pub fn requirements() -> Requirements {
  Requirements {
    protocol: 1,
    capabilities: BTreeSet::from(["runs".into()]),
  }
}
