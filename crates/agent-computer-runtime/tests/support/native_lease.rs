use super::*;
use crate::{Command, ComputerRuntime, Operation, Reply, RunBinding};
use agent_computer::*;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// A concurrent process spawn can briefly inherit another test's flock file
// description before exec closes it. Keep these physical-exclusion checks
// separate so the thread-drain assertion measures the worker's ownership.
static PROCESS_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Backend(DesktopIdentity);
impl DesktopBackend for Backend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    Ok(DesktopCapabilities {
      identity: self.0.clone(),
      interactive: true,
      capture: PermissionState::Granted,
      input: PermissionState::Granted,
      accessibility: PermissionState::Unsupported,
    })
  }
  fn observe(
    &mut self,
    _: &DesktopSession,
    _: ObservationLimits,
    _: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    Err(DesktopError::Unsupported("fixture".into()))
  }
  fn validate_snapshot(&mut self, _: &DesktopSnapshot) -> Result<(), DesktopError> {
    Err(DesktopError::Unsupported("fixture".into()))
  }
  fn execute(
    &mut self,
    _: &DesktopAction,
    _: &DesktopSnapshot,
    _: &ExecutionGuard,
  ) -> ActionOutcome {
    ActionOutcome::Rejected(DesktopError::Unsupported("fixture".into()))
  }
}
fn identity(node: &str) -> DesktopIdentity {
  DesktopIdentity {
    node_id: node.into(),
    user_session: "synthetic-native-session".into(),
  }
}
fn lease(runtime: &ComputerRuntime) -> LeaseToken {
  let result = runtime
    .handle(
      Command {
        binding: RunBinding {
          daemon_session_id: Uuid::new_v4().to_string(),
          run_id: Uuid::new_v4().to_string(),
          owner_epoch: 1,
        },
        operation: Operation::AcquireLease { ttl_ms: 300_000 },
      },
      CancellationToken::new(),
    )
    .expect("lease acquired");
  let Reply::Lease(lease) = result else {
    panic!("lease reply")
  };
  lease
}

#[test]
fn native_drop_revokes_stale_lease_but_worker_retains_physical_exclusion() {
  let _serial = PROCESS_TEST
    .lock()
    .expect("serialize native exclusion tests");
  let root = tempfile::tempdir().expect("temporary exclusion directory");
  let original = identity("first-node");
  let exclusion = NativeDesktopLease::acquire_in(root.path(), &original).expect("first attachment");
  let desktop = DesktopSession {
    id: Uuid::new_v4(),
    identity: original.clone(),
    scope: TargetScope::Window(7),
  };
  let registry = Arc::new(LeaseRegistry::default());
  let runtime = Arc::new(ComputerRuntime::new(
    desktop.clone(),
    Box::new(Backend(original.clone())),
    registry.clone(),
  ));
  let old = lease(&runtime);
  let cancellation = CancellationToken::new();
  let guard = AttachmentGuard::new(cancellation.clone(), &runtime, Some(exclusion.clone()));
  let (release, wait) = std::sync::mpsc::channel();
  let worker = std::thread::spawn(move || {
    let _exclusion = exclusion;
    wait.recv().expect("worker drain signal");
  });
  drop(guard);
  assert!(cancellation.is_cancelled());
  assert!(
    registry.validate(&old).is_err(),
    "old attachment lease revoked"
  );
  assert!(NativeDesktopLease::acquire_in(root.path(), &identity("second-node")).is_err());
  release.send(()).expect("drain worker");
  worker.join().expect("worker drained");
  let _next = NativeDesktopLease::acquire_in(root.path(), &identity("second-node"))
    .expect("replacement after drain");
  let next = ComputerRuntime::new(
    DesktopSession {
      id: Uuid::new_v4(),
      ..desktop
    },
    Box::new(Backend(original)),
    registry.clone(),
  );
  assert!(
    registry.validate(&lease(&next)).is_ok(),
    "stale native lease does not block replacement"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
      fs::metadata(root.path())
        .expect("directory")
        .permissions()
        .mode()
        & 0o777,
      0o700
    );
    let path = fs::read_dir(root.path())
      .expect("files")
      .next()
      .expect("file")
      .expect("entry")
      .path();
    assert_eq!(
      fs::metadata(path).expect("file").permissions().mode() & 0o777,
      0o600
    );
  }
}

#[test]
fn native_identity_excludes_another_process_despite_different_daemon_node() {
  let _serial = PROCESS_TEST
    .lock()
    .expect("serialize native exclusion tests");
  const CHILD_ROOT: &str = "LYONBROWN_NATIVE_LOCK_TEST_ROOT";
  const CHILD_BUSY: &str = "LYONBROWN_NATIVE_LOCK_EXPECT_BUSY";
  if let Some(root) = std::env::var_os(CHILD_ROOT) {
    let result = NativeDesktopLease::acquire_in(Path::new(&root), &identity("child-node"));
    assert_eq!(result.is_err(), std::env::var_os(CHILD_BUSY).is_some());
    return;
  }
  let root = tempfile::tempdir().expect("temporary exclusion directory");
  let exclusion = NativeDesktopLease::acquire_in(root.path(), &identity("parent-node"))
    .expect("parent attachment");
  let child = |busy: bool| {
    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command.args(["--exact", "native_lease::tests::native_identity_excludes_another_process_despite_different_daemon_node"])
      .env(CHILD_ROOT, root.path()).env_remove(CHILD_BUSY);
    if busy {
      command.env(CHILD_BUSY, "1");
    }
    assert!(command.status().expect("child lock check").success());
  };
  child(true);
  drop(exclusion);
  child(false);
}
