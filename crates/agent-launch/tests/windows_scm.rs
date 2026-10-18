#![cfg(windows)]

use agent_launch::{
  model::Registration,
  native::windows::{
    WindowsSetupError, command_line, current_user_sid, elevate_service_setup, validate_executable,
    validate_expected_user_sid,
  },
};
use anyhow::Result;

fn registration(directory: &std::path::Path) -> Result<Registration> {
  let executable = directory.join("Program Files").join("agentd.exe");
  let data_dir = directory.join("User Data");
  std::fs::create_dir_all(executable.parent().expect("fixture parent"))?;
  std::fs::create_dir_all(&data_dir)?;
  std::fs::write(&executable, b"service fixture")?;
  Ok(Registration {
    executable,
    data_dir,
    version: "1.0.0".parse()?,
    storage_schema_min: 1,
    storage_schema_max: 1,
  })
}

#[test]
fn scm_command_keeps_space_containing_paths_in_separate_arguments() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let registration = registration(directory.path())?;
  let name = format!("Lyonbrown.Agentd.{}", "a".repeat(64));
  let command = command_line(&registration, &name)?;
  assert_eq!(command.last(), Some(&0));
  let actual = String::from_utf16(&command[..command.len() - 1])?;
  assert_eq!(
    actual,
    format!(
      "\"{}\" --windows-service {name} --listen 127.0.0.1:7447 --data-dir \"{}\"",
      registration.executable.display(),
      registration.data_dir.display()
    )
  );
  Ok(())
}

#[test]
fn scm_staging_rejects_unscoped_identity_and_relative_or_missing_executable() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let mut registration = registration(directory.path())?;
  assert!(command_line(&registration, "org.lyonbrown.agentd").is_err());
  assert!(command_line(&registration, "Lyonbrown.Agentd.").is_err());
  let name = format!("Lyonbrown.Agentd.{}", "b".repeat(64));
  registration.executable = "agentd.exe".into();
  assert!(command_line(&registration, &name).is_err());
  registration.executable = directory.path().join("missing-agentd.exe");
  assert!(command_line(&registration, &name).is_err());
  Ok(())
}

#[test]
fn scm_preflight_rejects_user_directory_before_credential_ui_or_service_mutation() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let registration = registration(directory.path())?;
  let error =
    validate_executable(&registration).expect_err("user directory must not host SCM executable");
  assert!(error.to_string().contains("protected Program Files"));
  Ok(())
}

#[test]
fn setup_identity_guard_accepts_current_sid_and_rejects_any_identity_change() -> Result<()> {
  let sid = current_user_sid()?;
  assert!(sid.starts_with("S-1-"));
  validate_expected_user_sid(&sid)?;
  let error = validate_expected_user_sid("S-1-0-0 --register-service")
    .expect_err("changed or injected SID must be rejected");
  assert!(matches!(
    error.downcast_ref::<WindowsSetupError>(),
    Some(WindowsSetupError::SetupUserMismatch)
  ));
  Ok(())
}

#[test]
fn elevation_rejects_user_owned_helper_before_native_uac() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let helper = directory.path().join("agent-install.exe");
  std::fs::write(&helper, b"untrusted setup fixture")?;
  let error = elevate_service_setup(&helper, &current_user_sid()?)
    .expect_err("user helper cannot be elevated");
  assert!(
    error
      .to_string()
      .contains("protected installed Program Files")
  );
  Ok(())
}
