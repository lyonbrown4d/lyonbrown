use agent_local::{LifecycleLock, SecureRunDirectory, StartupLock};

#[test]
fn startup_lock_is_a_distinct_security_primitive() {
  let _ = std::any::TypeId::of::<StartupLock>();
}

#[test]
fn only_one_lifecycle_owner_can_hold_the_lock() {
  let parent = tempfile::tempdir().expect("temporary directory");
  let first_dir = SecureRunDirectory::open(parent.path().join("run")).expect("run directory");
  let second_dir = SecureRunDirectory::open(parent.path().join("run")).expect("run directory");
  let first = LifecycleLock::try_acquire(&first_dir)
    .expect("lock attempt")
    .expect("first owner");

  assert!(
    LifecycleLock::try_acquire(&second_dir)
      .expect("contended attempt")
      .is_none()
  );
  drop(first);
  assert!(
    LifecycleLock::try_acquire(&second_dir)
      .expect("retry")
      .is_some()
  );
}

#[test]
fn startup_election_and_lifecycle_ownership_can_overlap() {
  let parent = tempfile::tempdir().expect("temporary directory");
  let run_dir = SecureRunDirectory::open(parent.path().join("run")).expect("run directory");
  let lifecycle = LifecycleLock::try_acquire(&run_dir)
    .expect("lifecycle lock attempt")
    .expect("lifecycle owner");
  let startup = StartupLock::try_acquire(&run_dir)
    .expect("startup lock attempt")
    .expect("startup owner");

  drop(startup);
  drop(lifecycle);
}
