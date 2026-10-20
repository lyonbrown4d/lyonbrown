use crate::{
  BrowserPort,
  workbench::state::{Reply, Workbench},
};
use agent_protocol::v1::{BrowserFrame, BrowserSpace, BrowserTab};
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

fn frame(revision: u64) -> BrowserFrame {
  BrowserFrame {
    tab_id: "tab".into(),
    revision,
    sequence: revision,
    width: 1,
    height: 1,
    stride: 4,
    bgra8: vec![255; 4],
  }
}

fn deliver(view: &Entity<Workbench>, reply: Reply, cx: &mut TestAppContext) {
  cx.update(|cx| {
    let state = view.read(cx);
    state
      .sender
      .try_send((state.generation, reply))
      .expect("deliver completion");
  });
  // Completions wake GPUI directly; no timer advancement or native CEF is needed.
  cx.run_until_parked();
}

#[gpui_kit::test]
fn takeover_discards_every_obsolete_frame_completion(cx: &mut TestAppContext) {
  cx.update(crate::init);
  let window = cx.open_window(size(px(800.), px(600.)), |_, _| EmptyWindow);
  let runtime = Arc::new(
    tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .expect("fixture executor"),
  );
  let browser = fluxdi::Shared::new(BrowserPort {
    executable: "unused-browser".into(),
    profiles: "unused-profiles".into(),
  });
  let view = cx
    .update_window(window.into(), |_, window, cx| {
      cx.new(|cx| {
        Workbench::new(
          runtime,
          browser,
          fluxdi::Shared::new(crate::ComputerPort(Arc::new(
            agent_computer_runtime::NativeComputerHost,
          ))),
          window,
          cx,
        )
      })
    })
    .expect("workbench fixture");
  let previous = cx.update(|cx| view.read(cx).generation);
  cx.update_window(window.into(), |_, window, cx| {
    view.update(cx, |state, cx| {
      state.bind(None, Some("new-session".into()), Vec::new(), window, cx);
      state.pending_save = true;
      state
        .sender
        .try_send((previous, Reply::SaveError("obsolete save".into())))
        .expect("obsolete completion");
    });
  })
  .expect("change presentation generation");
  cx.run_until_parked();
  cx.update(|cx| {
    let state = view.read(cx);
    assert!(
      state.pending_save,
      "old completion cannot finish a new save"
    );
    assert!(state.error.is_none());
  });
  deliver(&view, Reply::SaveError("current save".into()), cx);
  cx.update(|cx| {
    view.update(cx, |state, _| {
      assert!(!state.pending_save);
      assert_eq!(state.error.as_deref(), Some("current save"));
      state.error = None;
    });
  });
  cx.update(|cx| {
    view.update(cx, |state, _| {
      state.tab = Some(BrowserTab {
        id: "tab".into(),
        ..Default::default()
      });
      state.frame_pending = true;
    })
  });
  let obsolete = cx.update(|cx| view.read(cx).frame_generation);
  deliver(
    &view,
    Reply::Control(BrowserSpace {
      human_control: true,
      ..Default::default()
    }),
    cx,
  );
  let current = cx.update(|cx| view.read(cx).frame_generation);
  assert!(current > obsolete);
  cx.update(|cx| {
    view.update(cx, |state, _| {
      assert!(state.frame.is_none());
      assert!(!state.frame_pending);
      // A new read now owns these pending flags. Old replies must not finish it.
      state.frame_pending = true;
      state.input_frame_pending = true;
    })
  });
  for reply in [
    Reply::Frame(obsolete, frame(1), true),
    Reply::NoFrame(obsolete),
    Reply::FrameError(obsolete, "obsolete read failed".into()),
  ] {
    deliver(&view, reply, cx);
    cx.update(|cx| {
      let state = view.read(cx);
      assert!(
        state.frame.is_none(),
        "takeover cannot display obsolete pixels"
      );
      assert!(
        state.frame_pending,
        "obsolete completion cannot finish the new read"
      );
      assert!(
        state.input_frame_pending,
        "obsolete pixels cannot authorize queued input"
      );
      assert!(
        state.browser_error.is_none(),
        "obsolete error cannot poison the new view"
      );
      assert_eq!(state.inputs_sent, 0);
    });
  }
  deliver(&view, Reply::Frame(current, frame(2), true), cx);
  cx.update(|cx| {
    let state = view.read(cx);
    assert_eq!(
      state.frame.as_ref().map(|(frame, _)| frame.revision),
      Some(2)
    );
    assert!(!state.frame_pending);
    assert!(!state.input_frame_pending);
    assert!(state.browser_error.is_none());
    assert_eq!(state.inputs_sent, 0);
  });
  let sender = cx.update(|cx| view.read(cx).sender.clone());
  cx.update(|_| drop(view));
  cx.run_until_parked();
  assert!(
    sender.is_closed(),
    "weak completion/frame tasks cannot retain a released workbench"
  );
}
