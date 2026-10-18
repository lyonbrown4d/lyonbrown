use crate::{
  BrowserPort,
  workbench::state::{Reply, Workbench},
};
use agent_protocol::v1::{BrowserFrame, BrowserSpace, BrowserTab};
use gpui_kit::{
  AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window, div, px, size,
};
use std::{sync::Arc, time::Duration};

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
      .send((state.generation, reply))
      .expect("deliver completion");
  });
  // Advance the existing workbench receiver deterministically, without network
  // timing, native CEF, or an additional production test interface.
  cx.run_until_parked();
  cx.executor().advance_clock(Duration::from_millis(80));
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
}
