use agent_local::{
  DiscoveryDescriptor, DiscoveryError, DiscoveryStore, ScopeFingerprint, SecretToken,
  SecureRunDirectory, StartupLock,
};

use std::net::SocketAddr;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use uuid::Uuid;

fn descriptor() -> DiscoveryDescriptor {
  DiscoveryDescriptor::new(
    "127.0.0.1:41000".parse::<SocketAddr>().expect("address"),
    100,
    Uuid::new_v4(),
    SecretToken::generate().expect("token"),
    ScopeFingerprint::parse(&"ef".repeat(32)).expect("scope"),
  )
  .expect("descriptor")
}

#[cfg(unix)]
#[test]
fn run_directory_and_discovery_file_are_private_and_atomic() {
  let parent = tempfile::tempdir().expect("temporary directory");
  let path = parent.path().join("run");
  let run_dir = SecureRunDirectory::open(&path).expect("secure run directory");
  assert!(run_dir.path().is_absolute());
  let store = DiscoveryStore::new(run_dir);
  let expected = descriptor();

  store.publish(&expected).expect("publish descriptor");
  let actual = store.read().expect("read descriptor");

  assert_eq!(actual.server_instance_id(), expected.server_instance_id());
  assert_eq!(
    std::fs::metadata(&path).expect("run metadata").mode() & 0o777,
    0o700
  );
  let discovery = path.join("agentd.json");
  let metadata = std::fs::metadata(discovery).expect("descriptor metadata");
  assert_eq!(metadata.mode() & 0o777, 0o600);
  assert_eq!(
    metadata.uid(),
    std::fs::metadata(&path).expect("run metadata").uid()
  );
}

#[cfg(unix)]
#[test]
fn run_directory_rejects_symlinks_and_insecure_modes() {
  use std::os::unix::fs::symlink;

  let parent = tempfile::tempdir().expect("temporary directory");
  let real = parent.path().join("real");
  std::fs::create_dir(&real).expect("real directory");
  std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).expect("permissions");
  let link = parent.path().join("link");
  symlink(&real, &link).expect("directory symlink");
  assert!(matches!(
    SecureRunDirectory::open(&link),
    Err(DiscoveryError::SymlinkRejected(_))
  ));

  let insecure = parent.path().join("insecure");
  std::fs::create_dir(&insecure).expect("insecure directory");
  std::fs::set_permissions(&insecure, std::fs::Permissions::from_mode(0o755)).expect("permissions");
  assert!(matches!(
    SecureRunDirectory::open(&insecure),
    Err(DiscoveryError::InsecurePermissions { .. })
  ));
}

#[cfg(unix)]
#[test]
fn discovery_read_rejects_a_symlink() {
  use std::os::unix::fs::symlink;

  let parent = tempfile::tempdir().expect("temporary directory");
  let run_dir = SecureRunDirectory::open(parent.path().join("run")).expect("secure run directory");
  let store = DiscoveryStore::new(run_dir);
  let outside = parent.path().join("outside.json");
  std::fs::write(&outside, descriptor().to_json().expect("json")).expect("outside descriptor");
  symlink(&outside, store.path()).expect("descriptor symlink");

  assert!(matches!(
    store.read(),
    Err(DiscoveryError::SymlinkRejected(_))
  ));
}

#[cfg(windows)]
#[test]
fn windows_run_directory_and_discovery_file_are_owner_only() {
  let parent = tempfile::tempdir().expect("temporary directory");
  let path = parent.path().join("missing").join("nested").join("run");
  let run_dir = SecureRunDirectory::open(&path).expect("secure run directory");
  let inherited = path.join("inherited.tmp");
  std::fs::write(&inherited, b"not a secret").expect("inherited file");
  assert_inherited_owner_only_acl(&inherited);
  std::fs::remove_file(&inherited).expect("remove inherited file");
  let store = DiscoveryStore::new(run_dir);
  let expected = descriptor();

  store.publish(&expected).expect("publish descriptor");
  let actual = store.read().expect("read descriptor");

  assert_eq!(actual.server_instance_id(), expected.server_instance_id());
  let replacement = descriptor();
  store.publish(&replacement).expect("replace descriptor");
  assert_eq!(
    store.read().expect("read replacement").server_instance_id(),
    replacement.server_instance_id()
  );
  assert_owner_only_acl(&path, true);
  assert_owner_only_acl(&store.path(), false);
}

#[cfg(windows)]
#[test]
fn windows_rejects_reparse_points_and_insecure_dacls() {
  use std::os::windows::fs::symlink_dir;

  let parent = tempfile::tempdir().expect("temporary directory");
  let real = parent.path().join("real");
  std::fs::create_dir(&real).expect("real directory");
  let link = parent.path().join("link");
  symlink_dir(&real, &link).expect("directory symlink");
  assert!(matches!(
    SecureRunDirectory::open(&link),
    Err(DiscoveryError::SymlinkRejected(_))
  ));

  let insecure = parent.path().join("insecure");
  std::fs::create_dir(&insecure).expect("insecure directory");
  grant_everyone_full_control(&insecure);
  assert!(matches!(
    SecureRunDirectory::open(&insecure),
    Err(DiscoveryError::InsecureAccessControl { .. })
  ));
}

#[cfg(windows)]
#[test]
fn windows_operations_remain_anchored_after_path_replacement() {
  let parent = tempfile::tempdir().expect("temporary directory");
  let path = parent.path().join("run");
  let moved = parent.path().join("moved");
  let run_dir = SecureRunDirectory::open(&path).expect("secure run directory");
  let expected = descriptor();

  std::fs::rename(&path, &moved).expect("rename open boundary");
  let replacement = SecureRunDirectory::open(&path).expect("replacement directory");
  let lock = StartupLock::try_acquire(&run_dir)
    .expect("anchored lock attempt")
    .expect("anchored lock");
  assert!(moved.join("agentd.start.lock").is_file());
  assert!(!path.join("agentd.start.lock").exists());
  drop(lock);
  let store = DiscoveryStore::new(run_dir);
  store.publish(&expected).expect("anchored publish");
  let actual = store.read().expect("anchored read");

  assert_eq!(actual.server_instance_id(), expected.server_instance_id());
  assert!(moved.join("agentd.json").is_file());
  assert!(!path.join("agentd.json").exists());
  drop(replacement);
}

#[cfg(windows)]
fn assert_inherited_owner_only_acl(path: &std::path::Path) {
  use std::fs::OpenOptions;
  use windows_permissions::constants::{
    AccessRights, AceFlags, AceType, SeObjectType, SecurityInformation,
  };
  use windows_permissions::{utilities, wrappers};

  let file = OpenOptions::new()
    .read(true)
    .open(path)
    .expect("open inherited file");
  let descriptor = wrappers::GetSecurityInfo(
    &file,
    SeObjectType::SE_FILE_OBJECT,
    SecurityInformation::Owner | SecurityInformation::Dacl,
  )
  .expect("security descriptor");
  let current = utilities::current_process_sid().expect("current process SID");
  let acl = descriptor.dacl().expect("DACL");
  assert_eq!(descriptor.owner(), Some(current.as_ref()));
  assert_eq!(acl.len(), 1);
  let ace = acl.get_ace(0).expect("inherited owner ACE");
  assert_eq!(ace.ace_type(), AceType::ACCESS_ALLOWED_ACE_TYPE);
  assert_eq!(ace.mask(), AccessRights::FileAllAccess);
  assert_eq!(ace.sid(), Some(current.as_ref()));
  assert_eq!(ace.flags(), AceFlags::Inherited);
}

#[cfg(windows)]
fn assert_owner_only_acl(path: &std::path::Path, directory: bool) {
  use std::fs::OpenOptions;
  use std::os::windows::fs::OpenOptionsExt as _;
  use windows_permissions::constants::SeObjectType;
  use windows_permissions::constants::{AccessRights, AceFlags, AceType, SecurityInformation};
  use windows_permissions::{utilities, wrappers};
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
  };

  let directory_flags = if directory {
    FILE_FLAG_BACKUP_SEMANTICS
  } else {
    0
  };
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | directory_flags)
    .open(path)
    .expect("open protected object");
  let descriptor = wrappers::GetSecurityInfo(
    &file,
    SeObjectType::SE_FILE_OBJECT,
    SecurityInformation::Owner | SecurityInformation::Dacl,
  )
  .expect("security descriptor");
  let current = utilities::current_process_sid().expect("current process SID");
  assert_eq!(descriptor.owner(), Some(current.as_ref()));
  let acl = descriptor.dacl().expect("DACL");
  assert_eq!(acl.len(), 1);
  let ace = acl.get_ace(0).expect("owner ACE");
  assert_eq!(ace.ace_type(), AceType::ACCESS_ALLOWED_ACE_TYPE);
  assert_eq!(ace.mask(), AccessRights::FileAllAccess);
  assert_eq!(ace.sid(), Some(current.as_ref()));
  let expected_flags = if directory {
    AceFlags::ContainerInherit | AceFlags::ObjectInherit
  } else {
    AceFlags::empty()
  };
  assert_eq!(ace.flags(), expected_flags);
  let sddl = wrappers::ConvertSecurityDescriptorToStringSecurityDescriptor(
    &descriptor,
    SecurityInformation::Dacl,
  )
  .expect("DACL SDDL");
  assert!(sddl.to_string_lossy().starts_with("D:P"));
}

#[cfg(windows)]
fn grant_everyone_full_control(path: &std::path::Path) {
  use std::fs::OpenOptions;
  use std::os::windows::fs::OpenOptionsExt as _;
  use windows_permissions::constants::{SeObjectType, SecurityInformation};
  use windows_permissions::{LocalBox, SecurityDescriptor, wrappers};
  use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
  };
  use windows_sys::Win32::Storage::FileSystem::{READ_CONTROL, WRITE_DAC};

  let mut file = OpenOptions::new()
    .access_mode(READ_CONTROL | WRITE_DAC)
    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
    .open(path)
    .expect("open insecure directory");
  let descriptor: LocalBox<SecurityDescriptor> =
    "D:P(A;OICI;FA;;;WD)".parse().expect("insecure DACL");
  wrappers::SetSecurityInfo(
    &mut file,
    SeObjectType::SE_FILE_OBJECT,
    SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
    None,
    None,
    descriptor.dacl(),
    None,
  )
  .expect("set insecure DACL");
}
