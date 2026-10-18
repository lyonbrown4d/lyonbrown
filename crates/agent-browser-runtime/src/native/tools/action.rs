use super::{call, invalidate_after_effect};
use crate::native::state::TabRecord;
use agent_browser::{Action, BrowserError, BrowserResult};
use cef::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(in crate::native) fn act(tab: &mut TabRecord, action: Action) -> BrowserResult<()> {
  let snapshot = tab
    .snapshot
    .as_ref()
    .filter(|snapshot| snapshot.id == action.snapshot_id && snapshot.revision == action.revision)
    .ok_or(BrowserError::StaleSnapshot)?;
  if tab.view.borrow().revision != action.revision {
    return Err(BrowserError::StaleSnapshot);
  }
  let backend = snapshot.elements.get(&action.element_ref).copied();
  let expected_accessibility = snapshot.accessibility.clone();
  let deadline = Instant::now() + Duration::from_millis(u64::from(action.timeout_ms));
  // DOM events do not cover every live property change (for example an input's
  // value). Recheck the bounded semantic context before producing any effect.
  let current = call(tab, "Accessibility.getFullAXTree", json!({}), deadline)?;
  if current["nodes"] != expected_accessibility || tab.view.borrow().revision != action.revision {
    tab.snapshot = None;
    let mut view = tab.view.borrow_mut();
    view.revision = view.revision.saturating_add(1);
    return Err(BrowserError::StaleSnapshot);
  }
  match action.kind.as_str() {
    "navigate" => {
      let frame = tab
        .browser
        .main_frame()
        .ok_or_else(|| BrowserError::Unavailable("CEF main frame missing".into()))?;
      effect_phase(tab, |tab| {
        frame.load_url(Some(&action.text.as_str().into()));
        tab.tab.url = action.text;
        Ok(())
      })
    }
    "click" => {
      let backend = backend
        .ok_or_else(|| BrowserError::InvalidInput("snapshot element is not actionable".into()))?;
      effect_phase(tab, |tab| {
        // Scrolling can run page handlers before geometry or mouse dispatch.
        call(
          tab,
          "DOM.scrollIntoViewIfNeeded",
          json!({ "backendNodeId": backend }),
          deadline,
        )?;
        let geometry = call(
          tab,
          "DOM.getBoxModel",
          json!({ "backendNodeId": backend }),
          deadline,
        )?;
        let quad = geometry["model"]["border"]
          .as_array()
          .ok_or_else(|| BrowserError::InvalidInput("element has no visible box".into()))?;
        let numbers: Vec<f64> = quad.iter().filter_map(Value::as_f64).collect();
        if numbers.len() != 8 {
          return Err(BrowserError::InvalidInput("element box is invalid".into()));
        }
        if tab.view.borrow().revision != action.revision {
          return Err(BrowserError::StaleSnapshot);
        }
        let (x, y) = (
          (numbers[0] + numbers[2] + numbers[4] + numbers[6]) / 4.,
          (numbers[1] + numbers[3] + numbers[5] + numbers[7]) / 4.,
        );
        call(
          tab,
          "Input.dispatchMouseEvent",
          json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 }),
          deadline,
        )?;
        call(
          tab,
          "Input.dispatchMouseEvent",
          json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 }),
          deadline,
        )?;
        Ok(())
      })
    }
    "fill" => {
      let backend = backend
        .ok_or_else(|| BrowserError::InvalidInput("snapshot element is not editable".into()))?;
      effect_phase(tab, |tab| {
        // Focus callbacks can produce effects before selection or text input.
        call(
          tab,
          "DOM.focus",
          json!({ "backendNodeId": backend }),
          deadline,
        )?;
        if tab.view.borrow().revision != action.revision {
          return Err(BrowserError::Uncertain);
        }
        let modifier = if cfg!(target_os = "macos") { 4 } else { 2 };
        call(
          tab,
          "Input.dispatchKeyEvent",
          json!({"type":"keyDown", "key":"a", "code":"KeyA", "windowsVirtualKeyCode":65, "modifiers":modifier}),
          deadline,
        )?;
        call(
          tab,
          "Input.dispatchKeyEvent",
          json!({"type":"keyUp", "key":"a", "code":"KeyA", "windowsVirtualKeyCode":65, "modifiers":modifier}),
          deadline,
        )?;
        call(
          tab,
          "Input.insertText",
          json!({ "text": action.text }),
          deadline,
        )?;
        Ok(())
      })
    }
    "press" => {
      let key = match action.text.as_str() {
        "Enter" => 13,
        "Escape" => 27,
        "Tab" => 9,
        "Backspace" => 8,
        _ => {
          return Err(BrowserError::InvalidInput(
            "supported keys: Enter, Escape, Tab, Backspace".into(),
          ));
        }
      };
      effect_phase(tab, |tab| {
        call(
          tab,
          "Input.dispatchKeyEvent",
          json!({"type":"keyDown","key":action.text,"windowsVirtualKeyCode":key}),
          deadline,
        )?;
        call(
          tab,
          "Input.dispatchKeyEvent",
          json!({"type":"keyUp","key":action.text,"windowsVirtualKeyCode":key}),
          deadline,
        )?;
        Ok(())
      })
    }
    _ => Err(BrowserError::InvalidInput(
      "unsupported browser action".into(),
    )),
  }
}

/// Once entered, an error cannot establish that the page saw no effect.
fn effect_phase(
  tab: &mut TabRecord,
  execute: impl FnOnce(&mut TabRecord) -> BrowserResult<()>,
) -> BrowserResult<()> {
  tab.snapshot = None;
  let result = execute(tab);
  // Advance only after execution: click still checks the authorized revision
  // after scrolling, before dispatching mouse input.
  invalidate_after_effect(tab);
  result.map_err(|_| BrowserError::Uncertain)
}
