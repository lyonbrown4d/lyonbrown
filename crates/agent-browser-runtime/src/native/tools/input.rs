use super::{TabRecord, call};
use agent_browser::{BrowserError, BrowserResult, ViewInput};
use cef::*;
use serde_json::json;
use std::time::{Duration, Instant};

pub(in crate::native) fn input(tab: &mut TabRecord, event: ViewInput) -> BrowserResult<()> {
  let deadline = Instant::now() + Duration::from_secs(5);
  let viewport = tab.view.borrow().viewport;
  let position = |x: i32, y: i32| -> BrowserResult<()> {
    if x < 0 || y < 0 || x >= viewport.0 as i32 || y >= viewport.1 as i32 {
      return Err(BrowserError::InvalidInput(
        "input coordinates are outside the current viewport".into(),
      ));
    }
    Ok(())
  };
  match event {
    ViewInput::MouseMove { x, y } => {
      position(x, y)?;
      call(
        tab,
        "Input.dispatchMouseEvent",
        json!({"type":"mouseMoved","x":x,"y":y}),
        deadline,
      )?;
    }
    ViewInput::MouseButton {
      x,
      y,
      button,
      pressed,
    } => {
      position(x, y)?;
      if pressed && let Some(host) = tab.browser.host() {
        // The GPUI focus belongs to another process. Transfer it explicitly to
        // the windowless Chromium widget before native pointer/text input.
        host.set_focus(1);
      }
      let button = match button {
        0 => "left",
        1 => "middle",
        2 => "right",
        _ => return Err(BrowserError::InvalidInput("invalid mouse button".into())),
      };
      call(
        tab,
        "Input.dispatchMouseEvent",
        json!({"type":if pressed {"mousePressed"} else {"mouseReleased"},"x":x,"y":y,"button":button,"clickCount":1}),
        deadline,
      )?;
    }
    ViewInput::Wheel {
      x,
      y,
      delta_x,
      delta_y,
    } => {
      position(x, y)?;
      call(
        tab,
        "Input.dispatchMouseEvent",
        json!({"type":"mouseWheel","x":x,"y":y,"deltaX":delta_x.clamp(-10000,10000),"deltaY":delta_y.clamp(-10000,10000)}),
        deadline,
      )?;
    }
    ViewInput::Key {
      virtual_key,
      modifiers,
      pressed,
    } => {
      if !(0..=65535).contains(&virtual_key) || modifiers & !15 != 0 {
        return Err(BrowserError::InvalidInput(
          "invalid key code or modifiers".into(),
        ));
      }
      call(
        tab,
        "Input.dispatchKeyEvent",
        json!({"type":if pressed {"keyDown"} else {"keyUp"},"windowsVirtualKeyCode":virtual_key,"modifiers":modifiers}),
        deadline,
      )?;
    }
    ViewInput::Text(text) => {
      if text.len() > 65536 {
        return Err(BrowserError::InvalidInput(
          "input text exceeds 64 KiB".into(),
        ));
      }
      call(tab, "Input.insertText", json!({"text":text}), deadline)?;
    }
    ViewInput::Resize { width, height } => {
      let size = (width as usize)
        .checked_mul(height as usize)
        .and_then(|size| size.checked_mul(4));
      if width == 0 || height == 0 || size.is_none_or(|size| size > agent_browser::MAX_FRAME_BYTES)
      {
        return Err(BrowserError::InvalidInput(
          "viewport exceeds CPU frame limit".into(),
        ));
      }
      {
        let mut view = tab.view.borrow_mut();
        view.viewport = (width, height);
        view.revision = view.revision.saturating_add(1);
        view.frame = None;
      }
      tab.snapshot = None;
      let host = tab
        .browser
        .host()
        .ok_or_else(|| BrowserError::Unavailable("CEF browser host closed".into()))?;
      host.was_resized();
    }
  }
  Ok(())
}
