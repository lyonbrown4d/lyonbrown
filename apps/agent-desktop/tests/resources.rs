#[path = "support/resources.rs"]
mod resources;
mod support;
use gpui_kit::{
  AppContext as _, ScrollDelta, TestAppContext, point, px,
  test::{TestQueryExt as _, TestWindowExt as _},
};
use support::Harness;

#[gpui_kit::test]
async fn model_selection_is_persisted_for_the_selected_session(cx: &mut TestAppContext) {
  let harness = Harness::new(cx, Vec::new());
  let session = resources::session(&harness, cx).await;
  resources::page(&harness, 1, cx);
  resources::ready(&harness, cx, |window, _| {
    window
      .try_find(("model-select", 0usize))
      .is_some_and(|button| button.disabled() != Some(true))
  })
  .await;
  harness.click(("model-select", 0usize), cx);
  harness
    .wait(cx, |_| {
      harness
        .daemon
        .store
        .model_binding(&session)
        .expect("stored binding")
        .is_some()
    })
    .await;
  let binding = harness
    .daemon
    .store
    .model_binding(&session)
    .unwrap()
    .unwrap();
  assert_eq!(binding.model_id, "fixture-model");
  assert_eq!(binding.execution_id, "configured");
  resources::page(&harness, 4, cx);
  resources::ready(&harness, cx, |window, _| {
    window
      .try_find("runtime-session")
      .is_some_and(|row| row.label() == Some(session.as_str()))
  })
  .await;
  cx.update_window(harness.window.into(), |_, window, _| {
    assert_eq!(window.find("runtime-model").label(), Some("fixture-model"));
  })
  .unwrap();
  assert!(
    harness
      .runtime
      .block_on(harness.model.requests())
      .is_empty(),
    "selection must not request inference"
  );
}

#[gpui_kit::test]
async fn skill_confirmation_uses_inspected_hash_and_source_edits_revoke_it(
  cx: &mut TestAppContext,
) {
  let harness = Harness::new(cx, Vec::new());
  let session = resources::session(&harness, cx).await;
  let source = harness.directory.path().join("acceptance-skill");
  std::fs::create_dir(&source).unwrap();
  std::fs::write(
    source.join("SKILL.md"),
    "---\nname: acceptance-skill\ndescription: Desktop fixture\n---\nSynthetic instructions.\n",
  )
  .unwrap();
  resources::page(&harness, 3, cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("skill-source", cx);
    window.input(source.to_str().unwrap(), cx);
    window.click("skill-inspect", cx);
  })
  .unwrap();
  resources::ready(&harness, cx, |window, _| {
    window.find("skill-install").disabled() != Some(true)
  })
  .await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("skill-source", cx);
    window.press("end", cx);
    window.input("-changed", cx);
    window.render_frame(cx);
    assert_eq!(window.find("skill-install").disabled(), Some(true));
    window.press(&support::shortcut("a"), cx);
    window.input(source.to_str().unwrap(), cx);
    window.click("skill-inspect", cx);
  })
  .unwrap();
  resources::ready(&harness, cx, |window, _| {
    window.find("skill-install").disabled() != Some(true)
  })
  .await;
  let expected = harness
    .runtime
    .block_on(
      harness
        .client
        .inspect_skill(source.to_str().unwrap(), "daemon_user", &session),
    )
    .unwrap()
    .skill
    .unwrap()
    .content_sha256;
  // Change files after inspection: agentd rejects the stale snapshot, without installing it.
  std::fs::write(source.join("extra.txt"), "Changed after inspection").unwrap();
  harness.click("skill-install", cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  assert!(
    harness
      .runtime
      .block_on(harness.client.list_skills("daemon_user", &session))
      .unwrap()
      .is_empty()
  );
  harness.click("skill-inspect", cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("skill-install").disabled() != Some(true)
  })
  .await;
  harness.click("skill-install", cx);
  resources::ready(&harness, cx, |window, _| {
    window
      .try_find(("skill-remove", 0usize))
      .is_some_and(|button| button.disabled() != Some(true))
  })
  .await;
  let listed = harness
    .runtime
    .block_on(harness.client.list_skills("daemon_user", &session))
    .unwrap();
  assert_eq!(listed.len(), 1);
  assert_ne!(listed[0].content_sha256, expected);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.scroll(
      "resource-content",
      ScrollDelta::Pixels(point(px(0.), px(-600.))),
      cx,
    );
    window.click(("skill-remove", 0usize), cx);
  })
  .unwrap();
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  assert!(
    harness
      .runtime
      .block_on(harness.client.list_skills("daemon_user", &session))
      .unwrap()
      .is_empty()
  );
}

#[gpui_kit::test]
async fn provider_environment_is_metadata_until_explicit_upload(cx: &mut TestAppContext) {
  const SECRET: &str = "synthetic-desktop-credential-never-render";
  // Set environment before constructing any runtime, in an isolated child process.
  if std::env::var_os("LYONBROWN_PROVIDER_UI_CHILD").is_none() {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
      .args([
        "--exact",
        "provider_environment_is_metadata_until_explicit_upload",
        "--nocapture",
      ])
      .env("LYONBROWN_PROVIDER_UI_CHILD", "1")
      .env("OPENAI_API_KEY", SECRET)
      .status()
      .expect("isolated credential UI test");
    assert!(status.success());
    return;
  }
  let harness = Harness::new(cx, Vec::new());
  resources::session(&harness, cx).await;
  resources::page(&harness, 2, cx);
  resources::ready(&harness, cx, |window, _| {
    window
      .try_find(("provider-upload", 0usize))
      .is_some_and(|button| button.disabled() != Some(true))
  })
  .await;
  let before = harness
    .runtime
    .block_on(harness.client.list_providers())
    .unwrap();
  assert_ne!(
    before
      .iter()
      .find(|provider| provider.id == "openai")
      .unwrap()
      .credential_source,
    "uploaded_encrypted"
  );
  cx.update_window(harness.window.into(), |_, window, _| {
    for element in window.elements() {
      assert!(
        !format!("{:?}", element).contains(SECRET),
        "credential value must not enter UI metadata"
      );
    }
  })
  .unwrap();
  harness.click(("provider-upload", 0usize), cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  let uploaded = harness
    .runtime
    .block_on(harness.client.list_providers())
    .unwrap();
  assert_eq!(
    uploaded
      .iter()
      .find(|provider| provider.id == "openai")
      .unwrap()
      .credential_source,
    "uploaded_encrypted"
  );
  harness.click(("provider-remove", 0usize), cx);
  resources::ready(&harness, cx, |window, _| {
    window.find("resource-refresh").disabled() != Some(true)
  })
  .await;
  let removed = harness
    .runtime
    .block_on(harness.client.list_providers())
    .unwrap();
  assert_ne!(
    removed
      .iter()
      .find(|provider| provider.id == "openai")
      .unwrap()
      .credential_source,
    "uploaded_encrypted"
  );
}
