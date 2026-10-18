use sha2::{Digest, Sha256};

use crate::*;

/// Versioned typed encoding: fixed-width little-endian integers and
/// length-prefixed bytes. Pixels are hashed directly, without another image copy.
pub(crate) fn content_digest(
  session: &DesktopSession,
  observation: &DesktopObservation,
) -> Result<[u8; 32], DesktopError> {
  let mut hash = ContentHash(Sha256::new());
  hash.bytes(b"lyonbrown-desktop-content-v1")?;
  hash.bytes(session.id.as_bytes())?;
  hash.bytes(session.identity.node_id.as_bytes())?;
  hash.bytes(session.identity.user_session.as_bytes())?;
  match session.scope {
    TargetScope::Display(id) => {
      hash.byte(0);
      hash.u32(id);
    }
    TargetScope::Window(id) => {
      hash.byte(1);
      hash.u32(id);
    }
  }
  hash.bounds(observation.bounds);
  hash.u32(observation.screenshot.width);
  hash.u32(observation.screenshot.height);
  hash.bytes(&observation.screenshot.rgba)?;
  hash.length(observation.windows.len())?;
  for window in &observation.windows {
    hash.u32(window.id);
    hash.u32(window.process_id);
    hash.bytes(window.title.as_bytes())?;
    hash.bounds(window.bounds);
    hash.byte(u8::from(window.focused));
  }
  hash.byte(u8::from(observation.windows_truncated));
  hash.byte(match observation.accessibility.state {
    PermissionState::Granted => 0,
    PermissionState::Denied => 1,
    PermissionState::Unsupported => 2,
    PermissionState::Unavailable => 3,
  });
  hash.length(observation.accessibility.nodes.len())?;
  for node in &observation.accessibility.nodes {
    hash.length(node.index)?;
    match node.parent {
      Some(parent) => {
        hash.byte(1);
        hash.length(parent)?;
      }
      None => hash.byte(0),
    }
    hash.bytes(node.role.as_bytes())?;
    hash.bytes(node.label.as_bytes())?;
  }
  hash.byte(u8::from(observation.accessibility.truncated));
  Ok(hash.0.finalize().into())
}

struct ContentHash(Sha256);

impl ContentHash {
  fn byte(&mut self, value: u8) {
    self.0.update([value]);
  }
  fn u32(&mut self, value: u32) {
    self.0.update(value.to_le_bytes());
  }
  fn length(&mut self, value: usize) -> Result<(), DesktopError> {
    let value = u64::try_from(value).map_err(|_| DesktopError::ContentDigestUnavailable)?;
    self.0.update(value.to_le_bytes());
    Ok(())
  }
  fn bytes(&mut self, value: &[u8]) -> Result<(), DesktopError> {
    self.length(value.len())?;
    self.0.update(value);
    Ok(())
  }
  fn bounds(&mut self, value: Bounds) {
    self.0.update(value.x.to_le_bytes());
    self.0.update(value.y.to_le_bytes());
    self.u32(value.width);
    self.u32(value.height);
  }
}
