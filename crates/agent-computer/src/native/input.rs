use enigo::{
  Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, NewConError, Settings,
};

use crate::*;

pub(super) fn execute(
  action: &DesktopAction,
  guard: &ExecutionGuard,
  content_check: impl FnMut() -> Result<(), DesktopError>,
  session_check: impl FnMut() -> Result<(), DesktopError>,
) -> ActionOutcome {
  let settings = Settings {
    open_prompt_to_get_permissions: false,
    release_keys_when_dropped: false,
    ..Settings::default()
  };
  let mut input = match Enigo::new(&settings) {
    Ok(input) => NativeInput(input),
    Err(NewConError::NoPermission) => {
      return ActionOutcome::Rejected(DesktopError::PermissionDenied("desktop input".into()));
    }
    Err(_) => {
      return ActionOutcome::Rejected(DesktopError::BackendUnavailable(
        "native input connection failed".into(),
      ));
    }
  };
  execute_with_input(action, guard, &mut input, content_check, session_check)
}

// The injected port keeps input sequencing testable without OS input or prompts.
trait InputPort {
  fn move_pointer(&mut self, x: i32, y: i32) -> Result<(), ()>;
  fn step(&mut self, action: &DesktopAction) -> Result<(), ()>;
  fn character(&mut self, character: char) -> Result<(), ()>;
}

fn execute_with_input(
  action: &DesktopAction,
  guard: &ExecutionGuard,
  input: &mut impl InputPort,
  mut content_check: impl FnMut() -> Result<(), DesktopError>,
  mut session_check: impl FnMut() -> Result<(), DesktopError>,
) -> ActionOutcome {
  if let Err(error) = content_check().and_then(|()| guard.checkpoint()) {
    return ActionOutcome::Rejected(error);
  }
  // A failing library call can follow a partial OS write. Even movement is input.
  if let Some((x, y)) = action.point() {
    if input.move_pointer(x, y).is_err() {
      return uncertain();
    }
    // Hover can change a click/scroll target, so recheck full content before the
    // next side effect. A move alone has no subsequent target to authorize.
    let checked = match action {
      DesktopAction::Click { .. } | DesktopAction::Scroll { .. } => content_check(),
      _ => session_check(),
    };
    if let Err(error) = checked.and_then(|()| guard.checkpoint()) {
      return ActionOutcome::Uncertain(error);
    }
  }
  if let DesktopAction::Text { text } = action {
    for character in text.chars() {
      // Expected pixels/AX changes from preceding characters must not truncate
      // approved text. Session, permission, focus, geometry and lease still hold.
      if let Err(error) = session_check().and_then(|()| guard.checkpoint()) {
        return ActionOutcome::Uncertain(error);
      }
      if input.character(character).is_err() {
        return uncertain();
      }
    }
  } else if input.step(action).is_err() {
    return uncertain();
  }
  // Input is expected to change content. Check only safety after dispatch;
  // checking the old digest here would turn successful input into Uncertain.
  match session_check().and_then(|()| guard.checkpoint()) {
    Ok(()) => ActionOutcome::Completed,
    Err(error) => ActionOutcome::Uncertain(error),
  }
}

struct NativeInput(Enigo);

impl InputPort for NativeInput {
  fn move_pointer(&mut self, x: i32, y: i32) -> Result<(), ()> {
    self.0.move_mouse(x, y, Coordinate::Abs).map_err(|_| ())
  }

  fn step(&mut self, action: &DesktopAction) -> Result<(), ()> {
    let result = match action {
      DesktopAction::Move { .. } | DesktopAction::Text { .. } => Ok(()),
      DesktopAction::Click { button, .. } => self.0.button(
        match button {
          MouseButton::Left => Button::Left,
          MouseButton::Middle => Button::Middle,
          MouseButton::Right => Button::Right,
        },
        Direction::Click,
      ),
      DesktopAction::Scroll { axis, lines, .. } => self.0.scroll(
        *lines,
        match axis {
          ScrollAxis::Vertical => Axis::Vertical,
          ScrollAxis::Horizontal => Axis::Horizontal,
        },
      ),
      DesktopAction::Key { key } => self.0.key(native_key(*key), Direction::Click),
    };
    result.map_err(|_| ())
  }

  fn character(&mut self, character: char) -> Result<(), ()> {
    self
      .0
      .text(character.encode_utf8(&mut [0; 4]))
      .map_err(|_| ())
  }
}

fn uncertain() -> ActionOutcome {
  ActionOutcome::Uncertain(DesktopError::BackendUnavailable(
    "native input result is unknown".into(),
  ))
}

fn native_key(key: DesktopKey) -> Key {
  match key {
    DesktopKey::Enter => Key::Return,
    DesktopKey::Tab => Key::Tab,
    DesktopKey::Escape => Key::Escape,
    DesktopKey::Space => Key::Space,
    DesktopKey::Backspace => Key::Backspace,
    DesktopKey::Delete => Key::Delete,
    DesktopKey::Up => Key::UpArrow,
    DesktopKey::Down => Key::DownArrow,
    DesktopKey::Left => Key::LeftArrow,
    DesktopKey::Right => Key::RightArrow,
    DesktopKey::Home => Key::Home,
    DesktopKey::End => Key::End,
    DesktopKey::PageUp => Key::PageUp,
    DesktopKey::PageDown => Key::PageDown,
  }
}

#[cfg(test)]
#[path = "../../tests/support/native_input.rs"]
mod tests;
