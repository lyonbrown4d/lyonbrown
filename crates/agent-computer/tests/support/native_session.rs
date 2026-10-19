use super::*;

#[test]
fn observed_lock_or_unknown_state_cannot_resume_the_same_attachment() {
  for error in [
    DesktopError::Locked,
    DesktopError::SessionStateUnknown,
    DesktopError::SessionUnavailable,
  ] {
    let guard = SessionGuard::default();
    assert_eq!(guard.check("original", || Ok("original".into())), Ok(()));
    assert_eq!(guard.check("original", || Err(error.clone())), Err(error));
    assert_eq!(
      guard.check("original", || Ok("original".into())),
      Err(DesktopError::SessionUnavailable)
    );
    let fresh = SessionGuard::default();
    assert_eq!(fresh.check("original", || Ok("original".into())), Ok(()));
  }
}

#[test]
fn session_switch_revokes_even_if_original_login_becomes_active_again() {
  let guard = SessionGuard::default();
  assert_eq!(
    guard.check("original", || Ok("other".into())),
    Err(DesktopError::SessionUnavailable)
  );
  assert_eq!(
    guard.check("original", || Ok("original".into())),
    Err(DesktopError::SessionUnavailable)
  );
}
