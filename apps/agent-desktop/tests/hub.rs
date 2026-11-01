#[path = "support/hub.rs"]
mod hub;
#[path = "support/resources.rs"]
mod resources;
mod support;
use gpui_kit::{
  AppContext as _, TestAppContext,
  test::{TestQueryExt as _, TestWindowExt as _},
};
use hub::Hub;
use std::sync::atomic::Ordering;
use support::Harness;

fn request(action: &str, id: &str) -> agent_protocol::v1::HubRequest {
  agent_protocol::v1::HubRequest {
    action: action.into(),
    id: id.into(),
    ..Default::default()
  }
}

fn receipt(harness: &Harness, cx: &mut TestAppContext) -> String {
  cx.update_window(harness.window.into(), |_, window, _| {
    window
      .find("model-hub-plan-receipt")
      .label()
      .expect("reviewed plan ID")
      .to_string()
  })
  .unwrap()
}

#[gpui_kit::test]
async fn hub_buttons_revoke_edited_plans_and_install_only_reviewed_artifacts(
  cx: &mut TestAppContext,
) {
  let hub = Hub::start();
  let harness = Harness::with_hub_endpoint(cx, hub.endpoint.clone());
  harness.connect(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  assert!(harness.snapshot(cx).selected_session.is_none());
  assert!(
    harness
      .runtime
      .block_on(harness.client.list_sessions())
      .unwrap()
      .is_empty()
  );
  resources::page(&harness, 1, cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  resources::ready(&harness, cx, |window, _| {
    window.try_find(("model-select", 0usize)).is_some()
  })
  .await;
  cx.update_window(harness.window.into(), |_, window, _| {
    assert_eq!(
      window.find(("model-select", 0usize)).disabled(),
      Some(true),
      "using a model requires an explicitly selected session"
    );
    assert!(window.find("model-selection-guidance").visible());
    assert!(
      window
        .elements()
        .iter()
        .any(|row| row.label() == Some("deepseek-chat")),
      "built-in models are listed without creating a session"
    );
  })
  .unwrap();
  hub::input(&harness, "model-hub-query", "test", cx);
  hub::click(&harness, "model-hub-search", cx);
  resources::ready(&harness, cx, |window, _| {
    window
      .try_find(("model-hub-result", 0usize))
      .is_some_and(|row| row.label() == Some("test/model") && row.disabled() != Some(true))
  })
  .await;
  hub::click(&harness, ("model-hub-result", 0usize), cx);
  hub::click(&harness, "model-hub-inspect", cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("model-hub-plan").disabled() != Some(true)
  })
  .await;
  hub::input(&harness, "model-hub-file", "model.gguf", cx);
  hub::click(&harness, "model-hub-plan", cx);
  resources::ready(&harness, cx, |window, _| {
    window
      .try_find("model-hub-install")
      .is_some_and(|button| button.disabled() != Some(true))
  })
  .await;
  let inspected_id = receipt(&harness, cx);
  assert!(!inspected_id.is_empty());
  assert!(
    harness
      .daemon
      .store
      .model_job(&inspected_id)
      .unwrap()
      .is_none()
  );
  assert_eq!(
    hub.downloads.load(Ordering::SeqCst),
    0,
    "inspection and planning cannot download weights"
  );

  // Both artifact selection and source revision edits revoke the displayed confirmation.
  hub::input(&harness, "model-hub-file", "changed.gguf", cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    assert!(window.try_find("model-hub-install").is_none());
  })
  .unwrap();
  hub::input(&harness, "model-hub-file", "model.gguf", cx);
  hub::click(&harness, "model-hub-plan", cx);
  resources::ready(&harness, cx, |window, _| {
    window.try_find("model-hub-install").is_some()
  })
  .await;
  hub::input(&harness, "model-hub-revision", "changed-revision", cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.render_frame(cx);
    assert!(window.try_find("model-hub-install").is_none());
    assert_eq!(window.find("model-hub-plan").disabled(), Some(true));
  })
  .unwrap();
  assert_eq!(hub.downloads.load(Ordering::SeqCst), 0);
  assert!(
    harness
      .daemon
      .store
      .model_job(&inspected_id)
      .unwrap()
      .is_none()
  );

  hub::input(&harness, "model-hub-revision", hub::REVISION, cx);
  hub::click(&harness, "model-hub-inspect", cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("model-hub-plan").disabled() != Some(true)
  })
  .await;
  hub::click(&harness, "model-hub-plan", cx);
  resources::ready(&harness, cx, |window, _| {
    window
      .try_find("model-hub-install")
      .is_some_and(|button| button.disabled() != Some(true))
  })
  .await;
  let confirmed_id = receipt(&harness, cx);
  assert_eq!(
    confirmed_id, inspected_id,
    "restored inputs must be re-inspected and reviewed"
  );
  hub::click(&harness, "model-hub-install", cx);
  harness
    .wait(cx, |_| {
      harness
        .daemon
        .store
        .model_job(&confirmed_id)
        .unwrap()
        .is_some_and(|job| job["state"] == "installed_unverified")
    })
    .await;
  assert_eq!(hub.downloads.load(Ordering::SeqCst), 1);
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  hub::click(&harness, ("model-job-action", 0usize), cx);
  resources::ready(&harness, cx, |window, _| {
    window.try_find(("model-job", 0usize)).is_some_and(|row| {
      row.label() == Some(format!("{confirmed_id} installed_unverified").as_str())
    })
  })
  .await;
  let status: serde_json::Value = serde_json::from_str(
    &harness
      .runtime
      .block_on(
        harness
          .client
          .hub_operation(request("status", &confirmed_id)),
      )
      .unwrap(),
  )
  .unwrap();
  assert_eq!(status["state"], "installed_unverified");
  assert_eq!(status["model"]["plan"]["id"], confirmed_id);
  assert_eq!(status["model"]["plan"]["revision"], hub::REVISION);
  assert_eq!(status["model"]["plan"]["entrypoint"], "model.gguf");
  let entrypoint = std::path::Path::new(status["model"]["entrypoint"].as_str().unwrap());
  assert_eq!(std::fs::read(entrypoint).unwrap(), hub::ARTIFACT);

  // Artifacts remain unverified until a separately authorized native load succeeds.
  let models = harness
    .runtime
    .block_on(harness.client.list_models(""))
    .unwrap();
  let installed = models
    .iter()
    .find(|model| model.id == confirmed_id)
    .unwrap();
  assert_eq!(installed.state, "installed_unverified");
  assert!(!installed.selected);
  hub::click(&harness, "model-hub-installed", cx);
  resources::ready(&harness, cx, |window, _| {
    window
      .try_find(("model-artifact-action", 2usize))
      .is_some_and(|button| button.disabled() != Some(true))
  })
  .await;
  hub::click(&harness, ("model-artifact-action", 2usize), cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  cx.update_window(harness.window.into(), |_, window, _| {
    assert!(
      window.try_find(("model-artifact-action", 2usize)).is_none(),
      "removed artifacts leave the displayed inventory"
    );
  })
  .unwrap();
  let remaining: serde_json::Value = serde_json::from_str(
    &harness
      .runtime
      .block_on(harness.client.hub_operation(request("installed", "")))
      .unwrap(),
  )
  .unwrap();
  assert!(
    remaining.as_array().unwrap().is_empty(),
    "remove must delete the persisted installed artifact"
  );
  assert!(!entrypoint.exists());
  assert!(harness.snapshot(cx).selected_session.is_none());
  assert!(
    harness
      .runtime
      .block_on(harness.client.list_sessions())
      .unwrap()
      .is_empty(),
    "global model management must not create a session"
  );
  assert!(harness.daemon.store.pending_runs().unwrap().is_empty());
  assert!(
    harness
      .runtime
      .block_on(harness.client.list_runs(""))
      .unwrap()
      .is_empty()
  );
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty()
  );
  // Creating a chat is explicit; daemon-global Hub jobs remain visible after catalog rebinding.
  harness.click("close-settings", cx);
  harness.click("new-session", cx);
  let selected = harness
    .wait(cx, |state| {
      state.selected_session.is_some() && !state.busy && !state.workbench.busy
    })
    .await
    .selected_session
    .unwrap();
  resources::page(&harness, 1, cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  cx.update_window(harness.window.into(), |_, window, _| {
    assert_eq!(
      window.find(("model-job", 0usize)).label(),
      Some(format!("{confirmed_id} installed_unverified").as_str()),
      "known daemon jobs survive chat selection"
    );
    assert_ne!(
      window.find(("model-select", 0usize)).disabled(),
      Some(true),
      "catalog refresh binds model selection to the explicit chat"
    );
  })
  .unwrap();
  assert!(
    harness
      .runtime
      .block_on(harness.client.list_runs(&selected))
      .unwrap()
      .is_empty()
  );
  assert!(harness.daemon.store.pending_runs().unwrap().is_empty());
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty()
  );
}
