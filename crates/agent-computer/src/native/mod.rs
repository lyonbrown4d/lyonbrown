//! xcap capture and Enigo input adapters. Linux currently requires an active,
//! unlocked X11/logind session; Wayland needs a separate portal grant lifecycle.
//! macOS capture/input code is present, but the native factory fails closed with
//! `SessionStateUnknown`: the public session probe cannot establish lock state.
mod capture;
mod input;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_accessibility;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

use crate::*;

pub struct NativeDesktopBackend {
  identity: DesktopIdentity,
  captured: Option<NativeCapture>,
}

struct NativeCapture {
  session: DesktopSession,
  limits: ObservationLimits,
  digest: [u8; 32],
  started: std::time::Instant,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct NativePermissionReport {
  pub session: Result<String, DesktopError>,
  pub capture: PermissionState,
  pub input: PermissionState,
  pub accessibility: PermissionState,
}

/// Read-only capability inspection, including failures before helper creation.
pub fn inspect_permissions() -> NativePermissionReport {
  let session = platform::session();
  let scoped = |state| {
    if session.is_err() && state == PermissionState::Granted {
      PermissionState::Unavailable
    } else {
      state
    }
  };
  NativePermissionReport {
    capture: scoped(platform::capture_permission()),
    input: scoped(platform::input_permission()),
    accessibility: scoped(platform::accessibility_permission()),
    session,
  }
}

impl NativeDesktopBackend {
  /// Read-only construction; no permission prompt, input injection or screenshot.
  pub fn new(node_id: String) -> Result<Self, DesktopError> {
    if node_id.is_empty() || node_id.len() > 256 {
      return Err(DesktopError::InvalidRequest("node identity".into()));
    }
    Ok(Self {
      captured: None,
      identity: DesktopIdentity {
        node_id,
        user_session: platform::session()?,
      },
    })
  }

  fn check_session(&self) -> Result<(), DesktopError> {
    if platform::session()? != self.identity.user_session {
      return Err(DesktopError::SessionUnavailable);
    }
    Ok(())
  }

  fn check_capture(&self) -> Result<(), DesktopError> {
    self.check_session()?;
    if platform::capture_permission() != PermissionState::Granted {
      return Err(DesktopError::PermissionDenied("screen capture".into()));
    }
    Ok(())
  }

  fn capture_observation(
    &self,
    session: &DesktopSession,
    limits: ObservationLimits,
    control: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    limits.validate()?;
    self.check_capture()?;
    if session.identity != self.identity {
      return Err(DesktopError::SessionUnavailable);
    }
    control.checkpoint(std::time::Instant::now())?;
    let mut observation = capture::observe(&session.scope, limits, control)?;
    observation.accessibility = platform::accessibility(&observation.windows, limits, control)?;
    self.check_capture()?;
    Ok(observation)
  }
  fn validate_session_snapshot(&self, snapshot: &DesktopSnapshot) -> Result<(), DesktopError> {
    self.check_capture()?;
    if snapshot.session.identity != self.identity {
      return Err(DesktopError::SessionUnavailable);
    }
    if platform::input_permission() != PermissionState::Granted {
      return Err(DesktopError::PermissionDenied("desktop input".into()));
    }
    capture::validate(snapshot)
  }

  fn validate_content_snapshot(&self, snapshot: &DesktopSnapshot) -> Result<(), DesktopError> {
    self.validate_session_snapshot(snapshot)?;
    let captured = self.captured.as_ref().ok_or(DesktopError::StaleSnapshot)?;
    if captured.session != snapshot.session
      || captured.digest
        != crate::content::content_digest(&snapshot.session, &snapshot.observation)?
    {
      return Err(DesktopError::StaleSnapshot);
    }
    // Capture and input are separate OS operations. Recheck all bounded content
    // immediately before input; this cannot promise an atomic OS capture+write.
    let control = OperationControl {
      cancellation: tokio_util::sync::CancellationToken::new(),
      // Revalidation must not extend the original snapshot's lifetime. Repeated
      // pre-input captures, including the hover check, share this same deadline.
      deadline: captured.started + std::time::Duration::from_secs(2),
    };
    control.checkpoint(std::time::Instant::now())?;
    let current = self.capture_observation(&captured.session, captured.limits, &control)?;
    control.checkpoint(std::time::Instant::now())?;
    if crate::content::content_digest(&captured.session, &current)? != captured.digest {
      return Err(DesktopError::StaleSnapshot);
    }
    control.checkpoint(std::time::Instant::now())?;
    if platform::input_permission() != PermissionState::Granted {
      return Err(DesktopError::PermissionDenied("desktop input".into()));
    }
    Ok(())
  }
}

impl DesktopBackend for NativeDesktopBackend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    self.check_session()?;
    Ok(DesktopCapabilities {
      identity: self.identity.clone(),
      interactive: true,
      capture: platform::capture_permission(),
      input: platform::input_permission(),
      accessibility: platform::accessibility_permission(),
    })
  }

  fn observe(
    &mut self,
    session: &DesktopSession,
    limits: ObservationLimits,
    control: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    let started = std::time::Instant::now();
    let observation = self.capture_observation(session, limits, control)?;
    self.captured = Some(NativeCapture {
      session: session.clone(),
      limits,
      digest: crate::content::content_digest(session, &observation)?,
      started,
    });
    Ok(observation)
  }

  fn validate_snapshot(&mut self, snapshot: &DesktopSnapshot) -> Result<(), DesktopError> {
    self.validate_content_snapshot(snapshot)
  }

  fn execute(
    &mut self,
    action: &DesktopAction,
    snapshot: &DesktopSnapshot,
    guard: &ExecutionGuard,
  ) -> ActionOutcome {
    if let Err(error) = guard.checkpoint() {
      return ActionOutcome::Rejected(error);
    }
    input::execute(
      action,
      guard,
      || self.validate_content_snapshot(snapshot),
      || self.validate_session_snapshot(snapshot),
    )
  }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
  use crate::*;
  pub fn session() -> Result<String, DesktopError> {
    Err(DesktopError::Unsupported("native desktop platform".into()))
  }
  pub fn capture_permission() -> PermissionState {
    PermissionState::Unsupported
  }
  pub fn input_permission() -> PermissionState {
    PermissionState::Unsupported
  }
  pub fn accessibility_permission() -> PermissionState {
    PermissionState::Unsupported
  }
  pub fn accessibility(
    _: &[WindowObservation],
    _: ObservationLimits,
    _: &OperationControl,
  ) -> Result<AccessibilityObservation, DesktopError> {
    Ok(AccessibilityObservation {
      state: PermissionState::Unsupported,
      nodes: vec![],
      truncated: false,
    })
  }
}
