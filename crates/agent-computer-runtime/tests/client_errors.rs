use agent_computer::DesktopError;
use agent_computer_runtime::computer_error_message;

#[test]
fn client_errors_preserve_known_categories_without_exposing_private_native_text() {
  let unknown_session = anyhow::Error::from(DesktopError::SessionStateUnknown)
    .context("private desktop title from an outer adapter");
  assert!(computer_error_message(&unknown_session).contains("SessionStateUnknown"));
  assert!(!computer_error_message(&unknown_session).contains("private desktop title"));
  for error in [
    anyhow::Error::from(DesktopError::PermissionDenied(
      "private permission detail".into(),
    )),
    anyhow::Error::from(DesktopError::BackendUnavailable(
      "private native title".into(),
    )),
    anyhow::anyhow!("private upstream credential"),
  ] {
    assert!(computer_error_message(&error).contains("ComputerRuntime unavailable"));
    assert!(!computer_error_message(&error).contains("private"));
  }
  assert!(computer_error_message(&DesktopError::StaleSnapshot.into()).contains("select again"));
}
