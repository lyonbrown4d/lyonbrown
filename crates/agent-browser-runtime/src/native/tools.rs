mod input;
mod paint;
pub(super) use input::input;
pub(super) use paint::request_paint_if_stale;

use super::state::{SnapshotCache, TabRecord};
use agent_browser::{Action, BrowserError, BrowserResult, Snapshot};
use cef::*;
use serde_json::{Value, json};
use std::{
  collections::BTreeMap,
  sync::atomic::{AtomicI32, Ordering},
  time::{Duration, Instant},
};

static NEXT_MESSAGE: AtomicI32 = AtomicI32::new(1);

pub(super) fn call(
  tab: &mut TabRecord,
  method: &str,
  parameters: Value,
  deadline: Instant,
) -> BrowserResult<Value> {
  let id = NEXT_MESSAGE.fetch_add(1, Ordering::Relaxed);
  if id <= 0 {
    return Err(BrowserError::Unavailable(
      "CEF DevTools request identity exhausted".into(),
    ));
  }
  let bytes = serde_json::to_vec(&json!({ "id": id, "method": method, "params": parameters }))
    .map_err(|error| BrowserError::Protocol(error.to_string()))?;
  let host = tab
    .browser
    .host()
    .ok_or_else(|| BrowserError::Unavailable("CEF browser host closed".into()))?;
  tab.view.borrow_mut().pending_id = Some(id);
  if host.send_dev_tools_message(Some(&bytes)) != 1 {
    tab.view.borrow_mut().pending_id = None;
    return Err(BrowserError::Unavailable(
      "CEF rejected DevTools command".into(),
    ));
  }
  loop {
    crate::native::pump();
    let response = {
      let mut view = tab.view.borrow_mut();
      if let Some(error) = view.pending_error.take() {
        view.pending_id = None;
        return Err(error);
      }
      let response = view.results.remove(&id);
      if response.is_some() {
        view.pending_id = None;
      }
      response
    };
    if let Some(message) = response {
      if message.get("error").is_some() {
        return Err(BrowserError::InvalidInput(
          "Chromium rejected the browser operation".into(),
        ));
      }
      return Ok(message["result"].clone());
    }
    if Instant::now() >= deadline {
      tab.view.borrow_mut().pending_id = None;
      return Err(if method.starts_with("Input.") || method == "DOM.focus" {
        BrowserError::Uncertain
      } else {
        BrowserError::Timeout
      });
    }
    std::thread::sleep(Duration::from_millis(4));
  }
}

pub(super) fn observe(tab: &mut TabRecord, screenshot: bool) -> BrowserResult<Snapshot> {
  let deadline = Instant::now() + Duration::from_secs(10);
  if !tab.view.borrow().dom_ready {
    call(tab, "DOM.enable", json!({}), deadline)?;
    call(tab, "Accessibility.enable", json!({}), deadline)?;
    // Subscribe the document once. Re-pushing AX backend nodes on every observation
    // emits frontend DOM notifications and invalidates otherwise stable snapshots.
    call(
      tab,
      "DOM.getDocument",
      json!({"depth":-1,"pierce":true}),
      deadline,
    )?;
    tab.view.borrow_mut().dom_ready = true;
  }
  let tree = call(tab, "Accessibility.getFullAXTree", json!({}), deadline)?;
  let nodes = tree["nodes"]
    .as_array()
    .ok_or_else(|| BrowserError::Protocol("Chromium accessibility response has no nodes".into()))?;
  let observed_revision = tab.view.borrow().revision;
  let mut elements = BTreeMap::new();
  let semantic: Vec<_> = nodes
    .iter()
    .enumerate()
    .filter(|(_, node)| node["ignored"] != true)
    .map(|(index, node)| {
      let reference = format!("e{index}");
      if let Some(backend) = node["backendDOMNodeId"].as_i64() {
        elements.insert(reference.clone(), backend);
      }
      json!({ "ref": reference, "nodeId": node["nodeId"], "role": node["role"]["value"],
      "name": node["name"]["value"], "value": node["value"]["value"],
      "properties": node["properties"], "children": node["childIds"] })
    })
    .collect();
  let accessibility_json =
    serde_json::to_string(&semantic).map_err(|error| BrowserError::Protocol(error.to_string()))?;
  if accessibility_json.len() > agent_browser::MAX_AX_BYTES {
    return Err(BrowserError::InvalidInput(
      "accessibility tree exceeds 1 MiB; narrow the page before observing".into(),
    ));
  }
  if screenshot {
    paint::request_paint_if_stale(tab);
  }
  let view = tab.view.borrow();
  tracing::debug!(
    event = "browser_snapshot_ready",
    nodes = nodes.len(),
    loading = view.loading,
    revision = view.revision,
    observed_revision,
    has_frame = view.frame.is_some(),
    "CEF snapshot observation completed"
  );
  if view.revision != observed_revision {
    return Err(BrowserError::StaleSnapshot);
  }
  let id = uuid::Uuid::now_v7().to_string();
  let revision = view.revision;
  let frame = screenshot
    .then(|| view.frame.clone())
    .flatten()
    .filter(|frame| frame.revision == revision);
  tab.snapshot = Some(SnapshotCache {
    id: id.clone(),
    revision,
    elements,
    accessibility: tree["nodes"].clone(),
  });
  Ok(Snapshot {
    id,
    tab_id: tab.tab.id.clone(),
    revision,
    accessibility_json,
    frame,
  })
}

pub(super) fn act(tab: &mut TabRecord, action: Action) -> BrowserResult<()> {
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
      frame.load_url(Some(&action.text.as_str().into()));
      tab.tab.url = action.text;
    }
    "click" => {
      let backend = backend
        .ok_or_else(|| BrowserError::InvalidInput("snapshot element is not actionable".into()))?;
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
    }
    "fill" => {
      let backend = backend
        .ok_or_else(|| BrowserError::InvalidInput("snapshot element is not editable".into()))?;
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
    }
    _ => {
      return Err(BrowserError::InvalidInput(
        "unsupported browser action".into(),
      ));
    }
  }
  tab.snapshot = None;
  let mut view = tab.view.borrow_mut();
  view.revision = view.revision.saturating_add(1);
  drop(view);
  paint::request_paint_if_stale(tab);
  Ok(())
}
