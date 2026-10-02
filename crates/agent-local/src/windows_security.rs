use std::fs::{File, OpenOptions};
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use cap_std::fs::{Dir, OpenOptions as CapOpenOptions, OpenOptionsExt as _};
use windows_permissions::constants::{
  AccessRights, AceFlags, AceType, SeObjectType, SecurityInformation,
};
use windows_permissions::{LocalBox, SecurityDescriptor, utilities, wrappers};
use windows_sys::Win32::Storage::FileSystem::{
  FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
  FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
  READ_CONTROL, WRITE_DAC,
};

use crate::DiscoveryError;

#[derive(Clone, Copy)]
enum ObjectKind {
  Directory,
  File,
}

impl ObjectKind {
  fn flags(self) -> AceFlags {
    match self {
      Self::Directory => AceFlags::ContainerInherit | AceFlags::ObjectInherit,
      Self::File => AceFlags::empty(),
    }
  }

  fn sddl_flags(self) -> &'static str {
    match self {
      Self::Directory => "OICI",
      Self::File => "",
    }
  }
}

pub(crate) fn open_run_directory(path: &Path) -> Result<(PathBuf, File), DiscoveryError> {
  let path = std::path::absolute(path)
    .map_err(|error| DiscoveryError::io("resolving run directory path", error))?;
  reject_reparse_components(&path)?;
  let created = create_missing_directories(&path)?;
  reject_reparse_components(&path)?;
  let canonical = path
    .canonicalize()
    .map_err(|error| DiscoveryError::io("canonicalizing run directory", error))?;
  let mut handle = open_directory(&canonical)?;
  validate_type(&canonical, &handle, ObjectKind::Directory)?;
  validate_owner(&canonical, &handle)?;
  if created {
    protect(&canonical, &mut handle, ObjectKind::Directory)?;
  }
  validate_private(&canonical, &handle, ObjectKind::Directory)?;
  Ok((canonical, handle))
}

fn create_missing_directories(path: &Path) -> Result<bool, DiscoveryError> {
  let mut missing = Vec::new();
  let mut current = path;
  loop {
    match std::fs::symlink_metadata(current) {
      Ok(_) => break,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
        missing.push(current.to_path_buf());
        current = current.parent().ok_or_else(|| {
          DiscoveryError::io(
            "resolving run directory parent",
            std::io::Error::new(std::io::ErrorKind::NotFound, "no existing path ancestor"),
          )
        })?;
      }
      Err(error) => return Err(DiscoveryError::io("inspecting run directory parent", error)),
    }
  }
  let created_target = !missing.is_empty();
  missing.reverse();
  for directory in missing {
    reject_reparse_components(directory.parent().unwrap_or(&directory))?;
    match std::fs::create_dir(&directory) {
      Ok(()) => {}
      Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
      Err(error) => return Err(DiscoveryError::io("creating run directory parent", error)),
    }
    reject_reparse_components(&directory)?;
    let mut handle = open_directory(&directory)?;
    validate_type(&directory, &handle, ObjectKind::Directory)?;
    validate_owner(&directory, &handle)?;
    protect(&directory, &mut handle, ObjectKind::Directory)?;
  }
  Ok(created_target)
}

pub(crate) fn open_private_lock(
  directory: &Dir,
  file_name: &str,
  path: &Path,
) -> Result<File, DiscoveryError> {
  let mut options = private_file_options();
  options.create_new(true);
  let (mut file, created) = match directory.open_with(file_name, &options) {
    Ok(file) => (file.into_std(), true),
    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
      let file = directory
        .open_with(file_name, &private_file_options())
        .map_err(|error| DiscoveryError::io("opening lock file", error))?;
      (file.into_std(), false)
    }
    Err(error) => return Err(DiscoveryError::io("creating lock file", error)),
  };
  validate_type(path, &file, ObjectKind::File)?;
  validate_owner(path, &file)?;
  if created {
    protect(path, &mut file, ObjectKind::File)?;
  }
  validate_private(path, &file, ObjectKind::File)?;
  Ok(file)
}

pub(crate) fn create_private_file(
  directory: &Dir,
  file_name: &str,
  path: &Path,
) -> Result<File, DiscoveryError> {
  let mut options = private_file_options();
  options.create_new(true);
  let mut file = directory
    .open_with(file_name, &options)
    .map_err(|error| DiscoveryError::io("creating private file", error))?
    .into_std();
  validate_type(path, &file, ObjectKind::File)?;
  validate_owner(path, &file)?;
  protect(path, &mut file, ObjectKind::File)?;
  Ok(file)
}

pub(crate) fn validate_private_file(path: &Path, file: &File) -> Result<(), DiscoveryError> {
  validate_private(path, file, ObjectKind::File)
}

pub(crate) fn open_private_read(
  directory: &Dir,
  file_name: &str,
  path: &Path,
) -> Result<File, DiscoveryError> {
  let file = directory
    .open_with(file_name, &private_file_options())
    .map_err(|error| DiscoveryError::io("opening discovery descriptor", error))?
    .into_std();
  validate_type(path, &file, ObjectKind::File)?;
  validate_private(path, &file, ObjectKind::File)?;
  Ok(file)
}

fn open_directory(path: &Path) -> Result<File, DiscoveryError> {
  OpenOptions::new()
    .access_mode(READ_CONTROL | WRITE_DAC)
    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
    .open(path)
    .map_err(|error| DiscoveryError::io("opening run directory", error))
}

fn private_file_options() -> CapOpenOptions {
  let mut options = CapOpenOptions::new();
  options
    .read(true)
    .write(true)
    .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | WRITE_DAC)
    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
  options
}

fn protect(path: &Path, file: &mut File, kind: ObjectKind) -> Result<(), DiscoveryError> {
  let current = utilities::current_process_sid()
    .map_err(|error| DiscoveryError::io("resolving current Windows user", error))?;
  let sddl = format!("D:P(A;{};FA;;;{})", kind.sddl_flags(), current);
  let descriptor: LocalBox<SecurityDescriptor> = sddl
    .parse()
    .map_err(|error| DiscoveryError::io("building private Windows DACL", error))?;
  wrappers::SetSecurityInfo(
    file,
    SeObjectType::SE_FILE_OBJECT,
    SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
    None,
    None,
    descriptor.dacl(),
    None,
  )
  .map_err(|error| DiscoveryError::io("protecting Windows filesystem object", error))?;
  validate_private(path, file, kind)
}

fn validate_owner(path: &Path, file: &File) -> Result<(), DiscoveryError> {
  let descriptor = wrappers::GetSecurityInfo(
    file,
    SeObjectType::SE_FILE_OBJECT,
    SecurityInformation::Owner,
  )
  .map_err(|error| DiscoveryError::io("reading Windows owner", error))?;
  let current = utilities::current_process_sid()
    .map_err(|error| DiscoveryError::io("resolving current Windows user", error))?;
  if descriptor.owner() == Some(current.as_ref()) {
    Ok(())
  } else {
    Err(DiscoveryError::WrongOwner {
      path: path.to_path_buf(),
    })
  }
}

fn validate_private(path: &Path, file: &File, kind: ObjectKind) -> Result<(), DiscoveryError> {
  validate_owner(path, file)?;
  let descriptor = wrappers::GetSecurityInfo(
    file,
    SeObjectType::SE_FILE_OBJECT,
    SecurityInformation::Dacl,
  )
  .map_err(|error| DiscoveryError::io("reading Windows DACL", error))?;
  let current = utilities::current_process_sid()
    .map_err(|error| DiscoveryError::io("resolving current Windows user", error))?;
  let Some(acl) = descriptor.dacl() else {
    return insecure(path);
  };
  let Some(ace) = (acl.len() == 1).then(|| acl.get_ace(0)).flatten() else {
    return insecure(path);
  };
  if ace.ace_type() != AceType::ACCESS_ALLOWED_ACE_TYPE
    || ace.mask() != AccessRights::FileAllAccess
    || ace.flags() != kind.flags()
    || ace.sid() != Some(current.as_ref())
  {
    return insecure(path);
  }
  let sddl = wrappers::ConvertSecurityDescriptorToStringSecurityDescriptor(
    &descriptor,
    SecurityInformation::Dacl,
  )
  .map_err(|error| DiscoveryError::io("serializing Windows DACL", error))?;
  if !sddl.to_string_lossy().starts_with("D:P") {
    return insecure(path);
  }
  Ok(())
}

fn validate_type(path: &Path, file: &File, kind: ObjectKind) -> Result<(), DiscoveryError> {
  let metadata = file
    .metadata()
    .map_err(|error| DiscoveryError::io("reading Windows filesystem metadata", error))?;
  if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
    return Err(DiscoveryError::SymlinkRejected(path.to_path_buf()));
  }
  let valid = match kind {
    ObjectKind::Directory => metadata.is_dir(),
    ObjectKind::File => metadata.is_file(),
  };
  valid
    .then_some(())
    .ok_or_else(|| DiscoveryError::WrongFileType(path.to_path_buf()))
}

fn reject_reparse_components(path: &Path) -> Result<(), DiscoveryError> {
  let mut components = path.ancestors().collect::<Vec<_>>();
  components.reverse();
  for component in components {
    match std::fs::symlink_metadata(component) {
      Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
        return Err(DiscoveryError::SymlinkRejected(component.to_path_buf()));
      }
      Ok(_) => {}
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
      Err(error) => return Err(DiscoveryError::io("inspecting Windows path", error)),
    }
  }
  Ok(())
}

fn insecure<T>(path: &Path) -> Result<T, DiscoveryError> {
  Err(DiscoveryError::InsecureAccessControl {
    path: path.to_path_buf(),
  })
}
