use gpui_kit::AppContext as _;
mod support;
use gpui_kit::{TestAppContext, test::TestWindowExt as _};
use support::Harness;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn semantic_contains(harness: &Harness, cx: &mut TestAppContext, tab: &str, needle: &str) {
  let client = harness.client.clone();
  let observed_tab = tab.to_string();
  let needle = needle.to_string();
  let observed = harness.runtime.spawn(async move {
    let mut last = String::new();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
      loop {
        match client.observe_browser(&observed_tab, false).await {
          Ok(observation) if observation.accessibility_json.contains(&needle) => break,
          Ok(observation) => {
            last = observation.accessibility_json;
          }
          Err(error)
            if error
              .downcast_ref::<tonic::Status>()
              .is_some_and(|status| status.code() == tonic::Code::FailedPrecondition) => {}
          Err(error) => panic!("daemon semantic CEF observation: {error}"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
      }
    })
    .await;
    result.unwrap_or_else(|_| panic!("native semantic text {needle:?} timed out: {last}"));
  });
  // Keep the real GPUI event loop moving while the serialized native input queue
  // drains; blocking this thread would prevent mouse-up and text acknowledgments.
  harness.wait(cx, |_| observed.is_finished()).await;
  harness
    .runtime
    .block_on(observed)
    .expect("native observation worker");
}

#[gpui_kit::test]
#[ignore = "requires LYONBROWN_NATIVE_CLIENT pointing to an actual native-cef product binary"]
async fn native_browser_frames_human_input_and_takeover_cross_authenticated_daemon(
  cx: &mut TestAppContext,
) {
  assert!(
    std::env::var_os("LYONBROWN_NATIVE_CLIENT").is_some(),
    "provide an actual built native-cef executable"
  );
  let harness = Harness::new(cx, Vec::new());
  let listener = harness
    .runtime
    .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
    .expect("local Chromium fixture page");
  let url = format!("http://{}/", listener.local_addr().expect("page address"));
  let server=harness.runtime.spawn(async move {
    loop {
      let Ok((mut socket,_))=listener.accept().await else{break};
      tokio::spawn(async move {
        let mut request=[0;4096];let _=socket.read(&mut request).await;
        let page=r#"<!doctype html><meta charset="utf-8"><title>Native Browser</title><h1>CEF acceptance</h1><input aria-label="Human input" style="position:fixed;left:10%;top:40%;width:80%;height:20%;box-sizing:border-box">"#;
        let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/html;charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",page.len());
        let _=socket.write_all(response.as_bytes()).await;
      });
    }
  });
  harness.connect(cx);
  harness.show_workbench(cx);
  harness
    .wait(cx, |state| state.status.starts_with("Connected"))
    .await;
  harness.click("new-session", cx);
  harness
    .wait(cx, |state| {
      state.selected_session.is_some() && !state.busy && !state.workbench.busy
    })
    .await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("browser-url", cx);
    window.press(&support::shortcut("a"), cx);
    window.input("not a browser URL", cx);
    window.click("browser-open", cx);
    let pending = harness.view.read(cx).snapshot(cx);
    assert!(pending.workbench.busy);
    window.click("new-chat-tab", cx);
    let blocked = harness.view.read(cx).snapshot(cx);
    assert_eq!(blocked.tabs.len(), pending.tabs.len());
    assert_eq!(blocked.selected_session, pending.selected_session);
    assert!(
      blocked
        .error
        .as_deref()
        .is_some_and(|error| error.starts_with("Wait for the browser operation")),
      "an in-flight mutation keeps its original chat until acknowledgement"
    );
    window.press(&support::shortcut("w"), cx);
    assert_eq!(
      harness.view.read(cx).snapshot(cx).selected_session,
      pending.selected_session,
      "the active chat cannot close while its browser mutation is pending"
    );
    window.click("open-settings", cx);
    window.click("connect", cx);
    assert!(
      harness
        .view
        .read(cx)
        .snapshot(cx)
        .status
        .starts_with("Connected"),
      "pending browser ownership prevents replacing the daemon connection"
    );
    window.click("close-settings", cx);
  })
  .expect("invalid native browser navigation input");
  let failed = harness
    .wait(cx, |state| {
      !state.workbench.busy && state.workbench.browser_error.is_some()
    })
    .await;
  assert!(
    failed
      .workbench
      .browser_error
      .as_ref()
      .is_some_and(|error| error.contains("invalid browser request")),
    "malformed navigation must fail URL validation: {:?}",
    failed.workbench.browser_error
  );
  let space = failed
    .workbench
    .browser_space
    .expect("failed navigation retains its successfully created Space");
  assert!(failed.workbench.browser_tab.is_none());
  assert!(failed.workbench.browser_frame.is_none());
  harness.wait(cx, |state| state.error.is_none()).await;
  let original_chat = failed
    .tabs
    .iter()
    .find(|tab| tab.selected)
    .expect("original native chat")
    .id;
  harness.click("new-chat-tab", cx);
  let away = harness
    .wait(cx, |state| {
      state.selected_session.is_some()
        && state.selected_session != failed.selected_session
        && !state.busy
        && !state.workbench.busy
    })
    .await;
  assert!(away.workbench.browser_space.is_none());
  harness.click(("chat-tab", original_chat), cx);
  let restored = harness.wait(cx, |state| !state.workbench.busy).await;
  assert_eq!(restored.workbench.browser_space.as_ref(), Some(&space));
  assert!(restored.workbench.browser_tab.is_none());
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click("browser-url", cx);
    window.press(&support::shortcut("a"), cx);
    window.input(&url, cx);
    window.click("browser-open", cx);
  })
  .expect("native browser navigation input");
  harness
    .wait(cx, |state| {
      state
        .workbench
        .browser_frame
        .is_some_and(|frame| frame.0 < 1000)
    })
    .await;
  let spaces = harness
    .runtime
    .block_on(harness.client.browser_spaces())
    .expect("daemon spaces after deliberate navigation retry");
  assert_eq!(spaces.len(), 1, "retry reuses the original profile owner");
  assert_eq!(spaces[0].id, space);
  assert_eq!(harness.snapshot(cx).workbench.browser_space, Some(space));
  assert!(harness.snapshot(cx).workbench.browser_error.is_none());
  let tab = harness
    .snapshot(cx)
    .workbench
    .browser_tab
    .expect("native tab identity");
  semantic_contains(&harness, cx, &tab, "Human input").await;
  harness.click("browser-takeover", cx);
  harness
    .wait(cx, |state| {
      state.workbench.browser_human_control
        && state
          .workbench
          .browser_frame
          .is_some_and(|frame| Some((frame.0, frame.1)) == state.workbench.browser_viewport)
        && !state.workbench.browser_input_pending
        && state.workbench.browser_queued_inputs == 0
    })
    .await;
  cx.update_window(harness.window.into(), |_, window, cx| {
    eprintln!(
      "Native browser surface bounds: {:?}",
      window.find("browser-surface").bounds()
    );
    window.click("browser-surface", cx);
    // GPUI installs the focused IME handler during paint, as native input does.
    window.render_frame(cx);
    assert_eq!(
      window.find("browser-surface").focused(),
      Some(true),
      "browser native input surface takes focus"
    );
    window.input("human text", cx);
  })
  .expect("native mouse and text events into CEF surface");
  semantic_contains(&harness, cx, &tab, "human text").await;
  let state = harness.snapshot(cx);
  let (width, height, _) = state.workbench.browser_frame.expect("rendered native view");
  assert_eq!(
    state.workbench.browser_pointer,
    Some((width as i32 / 2, height as i32 / 2)),
    "native pointer coordinates track the rendered view"
  );
  harness.click("browser-takeover", cx);
  harness
    .wait(cx, |state| {
      !state.workbench.browser_human_control && state.workbench.browser_frame.is_some()
    })
    .await;
  let denied = harness.runtime.block_on(harness.client.browser_input(
    agent_protocol::v1::BrowserInput {
      tab_id: tab,
      kind: "text".into(),
      text: "unauthorized input".into(),
      ..Default::default()
    },
  ));
  assert!(
    denied.is_err(),
    "daemon independently enforces native takeover"
  );
  harness.click("browser-close", cx);
  harness
    .wait(cx, |state| {
      state.workbench.browser_space.is_none() && state.workbench.browser_frame.is_none()
    })
    .await;
  assert!(
    harness
      .runtime
      .block_on(harness.client.browser_spaces())
      .expect("daemon spaces")
      .is_empty()
  );
  server.abort();
}
