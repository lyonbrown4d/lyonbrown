mod action;
mod input;
mod paint;
pub(super) use action::act;
pub(super) use input::input;
pub(super) use paint::request_paint_if_stale;

use super::state::{SnapshotCache, TabRecord};
use agent_browser::{BrowserError, BrowserResult, Snapshot};
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

/// Retire semantic references and painted frames after a possible page effect.
pub(super) fn invalidate_after_effect(tab: &mut TabRecord) {
  tab.snapshot = None;
  let mut view = tab.view.borrow_mut();
  view.revision = view.revision.saturating_add(1);
  drop(view);
  paint::request_paint_if_stale(tab);
}
