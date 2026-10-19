use super::*;

fn active() -> SessionSnapshot {
  SessionSnapshot {
    on_console: Some(true),
    login_done: Some(true),
    lock_hint: None,
    identity: Some(uuid::Uuid::from_u128(42)),
  }
}

#[test]
fn unlocked_console_requires_explicit_lock_state_and_unique_login_identity() {
  let mut state = active();
  assert_eq!(
    state.validate(Some(false)),
    Ok(format!("macos:{}", uuid::Uuid::from_u128(42)))
  );
  assert_eq!(state.validate(None), Err(DesktopError::SessionStateUnknown));
  state.identity = None;
  assert_eq!(
    state.validate(Some(false)),
    Err(DesktopError::SessionStateUnknown)
  );
  state.identity = Some(uuid::Uuid::nil());
  assert_eq!(
    state.validate(Some(false)),
    Err(DesktopError::SessionStateUnknown)
  );
}

#[test]
fn lock_hints_and_inactive_login_never_authorize_control() {
  let mut state = active();
  assert_eq!(state.validate(Some(true)), Err(DesktopError::Locked));
  state.lock_hint = Some(true);
  assert_eq!(state.validate(Some(false)), Err(DesktopError::Locked));
  state.lock_hint = None;
  for flag in [None, Some(false)] {
    state.on_console = flag;
    assert_eq!(
      state.validate(Some(false)),
      Err(DesktopError::SessionUnavailable)
    );
    state.on_console = Some(true);
    state.login_done = flag;
    assert_eq!(
      state.validate(Some(false)),
      Err(DesktopError::SessionUnavailable)
    );
    state.login_done = Some(true);
  }
}
