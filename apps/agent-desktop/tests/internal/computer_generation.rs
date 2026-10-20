use crate::test_support as support;

use crate::{
  ComputerPort,
  computer::{ComputerPane, Reply},
};
use agent_computer::{Bounds, DesktopTarget, TargetScope};
use gpui_kit::{
  AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window, div, px, size,
};
use std::sync::Arc;

struct EmptyWindow;
impl Render for EmptyWindow {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    div()
  }
}

fn target() -> DesktopTarget {
  DesktopTarget {
    scope: TargetScope::Window(7),
    label: "Obsolete private title".into(),
    process_id: Some(42),
    bounds: Bounds {
      x: 0,
      y: 0,
      width: 2,
      height: 2,
    },
  }
}

fn deliver(view: &Entity<ComputerPane>, generation: u64, reply: Reply, cx: &mut TestAppContext) {
  cx.update(|cx| {
    view
      .read(cx)
      .sender
      .try_send((generation, reply))
      .expect("completion")
  });
  cx.run_until_parked();
}

#[gpui_kit::test]
fn scope_catalog_and_errors_from_old_session_connection_or_operation_cannot_update_new_view(
  cx: &mut TestAppContext,
) {
  cx.update(crate::init);
  let window = cx.open_window(size(px(800.), px(600.)), |_, _| EmptyWindow);
  let runtime = Arc::new(
    tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .expect("fixture runtime"),
  );
  let view = cx
    .update_window(window.into(), |_, window, cx| {
      cx.new(|cx| {
        ComputerPane::new(
          runtime,
          fluxdi::Shared::new(ComputerPort(Arc::new(
            agent_computer_runtime::NativeComputerHost,
          ))),
          window,
          cx,
        )
      })
    })
    .expect("pane");
  for boundary in 0..3 {
    let previous = cx.update(|cx| view.read(cx).generation);
    cx.update_window(window.into(), |_, window, cx| {
      view.update(cx, |view, cx| {
        match boundary {
          0 => view.bind(None, Some("different-chat".into()), window, cx),
          1 => view.reset_connection(window, cx),
          // A newer operation is independent of selected-chat identity.
          _ => view.generation += 1,
        }
        view.pending = true;
      })
    })
    .expect("change presentation lifetime");
    deliver(&view, previous, Reply::Targets(vec![target()]), cx);
    deliver(&view, previous, Reply::Error("obsolete error"), cx);
    cx.update(|cx| {
      let state = view.read(cx).snapshot(cx);
      assert!(state.targets.is_empty());
      assert!(state.selected_scope.is_none());
      assert!(state.bound_session.is_none());
      assert!(
        state.pending,
        "obsolete completion must not finish a new operation"
      );
      assert!(state.error.is_none());
    });
    let current = cx.update(|cx| view.read(cx).generation);
    deliver(&view, current, Reply::Error("current error"), cx);
    cx.update(|cx| {
      let state = view.read(cx).snapshot(cx);
      assert!(!state.pending);
      assert_eq!(state.error.as_deref(), Some("current error"));
    });
    cx.update_window(window.into(), |_, window, cx| {
      view.update(cx, |view, cx| view.reset_connection(window, cx));
    })
    .expect("clear previous operation state");
  }
  let sender = cx.update(|cx| view.read(cx).sender.clone());
  cx.update(|_| drop(view));
  cx.run_until_parked();
  assert!(
    sender.is_closed(),
    "weak completion/health tasks cannot retain a released pane"
  );
}

#[gpui_kit::test]
async fn obsolete_attachment_reply_is_dropped_and_revokes_authenticated_host(
  cx: &mut TestAppContext,
) {
  use agent_computer_runtime::ComputerHostFactory as _;
  use std::sync::atomic::Ordering;
  let factory = Arc::new(support::computer::Factory::default());
  let harness = support::Harness::with_computer(cx, factory.clone());
  let session = harness
    .runtime
    .block_on(
      harness.client.create_session(
        "attachment origin",
        harness
          .directory
          .path()
          .join("workspace")
          .to_str()
          .expect("workspace path"),
      ),
    )
    .expect("daemon session");
  let computer = cx.update(|cx| harness.view.read(cx).tasks.read(cx).computer.clone());
  cx.update_window(harness.window.into(), |_, window, cx| {
    computer.update(cx, |view, cx| {
      view.bind(
        Some(harness.client.clone()),
        Some(session.id.clone()),
        window,
        cx,
      )
    });
  })
  .expect("bind origin chat");
  let previous = cx.update(|cx| computer.read(cx).generation);
  let host = harness
    .runtime
    .block_on(factory.attach(
      harness.client.clone(),
      &session.id,
      support::computer::target(),
    ))
    .expect("real reverse channel");
  assert!(
    harness
      .runtime
      .block_on(harness.daemon.computer_runtime_info(&session.id))
      .expect("lookup")
      .is_some()
  );
  cx.update_window(harness.window.into(), |_, window, cx| {
    computer.update(cx, |view, cx| view.reset_connection(window, cx));
  })
  .expect("new connection lifetime");
  deliver(
    &computer,
    previous,
    Reply::Attached(session.id.clone(), support::computer::target(), host),
    cx,
  );
  harness
    .wait(cx, |state| {
      !state.workbench.computer.attached
        && state.workbench.computer.bound_session.is_none()
        && factory.dropped.load(Ordering::SeqCst) == 1
        && harness
          .runtime
          .block_on(harness.daemon.computer_runtime_info(&session.id))
          .expect("lookup")
          .is_none()
    })
    .await;
}
