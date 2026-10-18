use std::collections::VecDeque;

use accessibility_sys::*;
use core_foundation::{
  array::{CFArray, CFArrayRef},
  base::CFRange,
  base::{CFType, TCFType},
  string::{CFString, CFStringGetCharacters},
};

use crate::*;

fn unavailable() -> DesktopError {
  DesktopError::BackendUnavailable("accessibility observation unavailable".into())
}

fn element(value: &CFType) -> Result<AXUIElementRef, DesktopError> {
  // SAFETY: AXUIElementGetTypeID has no arguments. A checked CFType can be cast
  // to the opaque AX reference while its owning CFType remains alive.
  if value.type_of() != unsafe { AXUIElementGetTypeID() } {
    return Err(unavailable());
  }
  Ok(value.as_CFTypeRef() as AXUIElementRef)
}

fn attribute(value: &CFType, name: &str) -> Result<CFType, DesktopError> {
  let raw = element(value)?;
  let name = CFString::new(name);
  let mut result = std::ptr::null();
  // SAFETY: input element/type/attribute are checked and live; result is an
  // out-pointer. Copy transfers a +1 reference only on successful non-null return.
  let status =
    unsafe { AXUIElementCopyAttributeValue(raw, name.as_concrete_TypeRef(), &mut result) };
  if status != kAXErrorSuccess || result.is_null() {
    return Err(unavailable());
  }
  Ok(unsafe { CFType::wrap_under_create_rule(result) })
}

fn text(value: &CFType, name: &str) -> Result<String, DesktopError> {
  let string = attribute(value, name)?
    .downcast::<CFString>()
    .ok_or_else(unavailable)?;
  let length = string.char_len().min(512);
  let mut buffer = vec![0_u16; length as usize];
  // SAFETY: CF range is within the immutable string and buffer has `length`
  // UTF-16 units. This bounds allocation before copying potentially private text.
  unsafe {
    CFStringGetCharacters(
      string.as_concrete_TypeRef(),
      CFRange::init(0, length),
      buffer.as_mut_ptr(),
    )
  };
  Ok(String::from_utf16_lossy(&buffer))
}

fn children(value: &CFType, limit: usize) -> Result<Vec<CFType>, DesktopError> {
  if limit == 0 {
    return Ok(vec![]);
  }
  let raw = element(value)?;
  let name = CFString::new(kAXChildrenAttribute);
  let mut count = 0;
  // SAFETY: live checked element/name and a valid count output.
  let status =
    unsafe { AXUIElementGetAttributeValueCount(raw, name.as_concrete_TypeRef(), &mut count) };
  if status == kAXErrorAttributeUnsupported || status == kAXErrorNoValue {
    return Ok(vec![]);
  }
  if status != kAXErrorSuccess || count < 0 {
    return Err(unavailable());
  }
  if count == 0 {
    return Ok(vec![]);
  }
  let mut array: CFArrayRef = std::ptr::null();
  // SAFETY: bounded count and index; Copy transfers the returned array retain.
  let status = unsafe {
    AXUIElementCopyAttributeValues(
      raw,
      name.as_concrete_TypeRef(),
      0,
      count.min(limit as isize),
      &mut array,
    )
  };
  if status != kAXErrorSuccess || array.is_null() {
    return Err(unavailable());
  }
  let array: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(array) };
  Ok(array.iter().map(|item| (*item).clone()).collect())
}

pub(super) fn observe(
  windows: &[WindowObservation],
  limits: ObservationLimits,
  control: &OperationControl,
) -> Result<AccessibilityObservation, DesktopError> {
  let state = super::macos::accessibility_permission();
  let mut observation = AccessibilityObservation {
    state,
    nodes: vec![],
    truncated: false,
  };
  if state != PermissionState::Granted || limits.max_nodes == 0 {
    return Ok(observation);
  }
  let Some(window) = windows.iter().find(|w| w.focused) else {
    observation.state = PermissionState::Unavailable;
    return Ok(observation);
  };
  let result = tree(window, limits, control);
  match result {
    Ok((nodes, truncated)) => {
      observation.nodes = nodes;
      observation.truncated = truncated;
    }
    Err(DesktopError::Cancelled) => return Err(DesktopError::Cancelled),
    Err(DesktopError::DeadlineExceeded) => return Err(DesktopError::DeadlineExceeded),
    Err(_) => observation.state = PermissionState::Unavailable,
  }
  Ok(observation)
}

fn tree(
  window: &WindowObservation,
  limits: ObservationLimits,
  control: &OperationControl,
) -> Result<(Vec<AccessibilityNode>, bool), DesktopError> {
  let pid = i32::try_from(window.process_id).map_err(|_| unavailable())?;
  // SAFETY: constructor takes a plain PID and returns an owned AX element.
  let app = unsafe { AXUIElementCreateApplication(pid) };
  if app.is_null() {
    return Err(unavailable());
  }
  let app = unsafe { CFType::wrap_under_create_rule(app.cast()) };
  // SAFETY: owned live application AX object and a finite timeout.
  if unsafe { AXUIElementSetMessagingTimeout(element(&app)?, 0.05) } != kAXErrorSuccess {
    return Err(unavailable());
  }
  let focused = attribute(&app, kAXFocusedWindowAttribute)?;
  // Do not attach another application's/window's AX tree to the scoped image.
  if text(&focused, kAXTitleAttribute)? != window.title {
    return Err(unavailable());
  }
  let mut queue = VecDeque::from([(focused, None, 0)]);
  let mut nodes = Vec::new();
  let mut truncated = false;
  while let Some((value, parent, depth)) = queue.pop_front() {
    control.checkpoint(std::time::Instant::now())?;
    let raw = element(&value)?;
    // SAFETY: checked live AX object; finite per-call timeout prevents a hung
    // application from indefinitely retaining an observation request.
    if unsafe { AXUIElementSetMessagingTimeout(raw, 0.05) } != kAXErrorSuccess {
      return Err(unavailable());
    }
    let index = nodes.len();
    nodes.push(AccessibilityNode {
      index,
      parent,
      role: text(&value, kAXRoleAttribute)?,
      label: text(&value, kAXTitleAttribute).unwrap_or_default(),
    });
    let remaining = limits.max_nodes.saturating_sub(nodes.len() + queue.len());
    if depth >= limits.max_depth || remaining == 0 {
      truncated = true;
      continue;
    }
    let descendants = children(&value, remaining + 1)?;
    if descendants.len() > remaining {
      truncated = true;
    }
    queue.extend(
      descendants
        .into_iter()
        .take(remaining)
        .map(|child| (child, Some(index), depth + 1)),
    );
  }
  Ok((nodes, truncated))
}
