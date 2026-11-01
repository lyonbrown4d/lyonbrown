use super::{
  ResourcePane, ResourceSection,
  state::{Completion, Reply},
};
use gpui_kit::{
  AppContext as _, Context, IntoElement, Render, TestAppContext, Window, div, px, size,
};
use std::sync::Arc;

struct Empty;
impl Render for Empty {
  fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    div()
  }
}

#[gpui_kit::test]
fn scoped_replies_and_inspection_receipts_cannot_cross_lifetimes(cx: &mut TestAppContext) {
  cx.update(crate::init);
  let window = cx.open_window(size(px(900.), px(700.)), |_, _| Empty);
  let runtime = Arc::new(
    tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .unwrap(),
  );
  let pane = cx
    .update_window(window.into(), |_, window, cx| {
      cx.new(|cx| ResourcePane::new(ResourceSection::Skills, runtime, window, cx))
    })
    .unwrap();
  let old = cx.update(|cx| pane.read(cx).generation);
  cx.update(|cx| {
    pane.update(cx, |pane, cx| {
      pane.pending_mutations = 1;
      pane.bind(None, Some("new-session".into()), cx);
    })
  });
  let deliver = |generation, input_revision, mutation, result, cx: &mut TestAppContext| {
    cx.update(|cx| {
      pane
        .read(cx)
        .sender
        .try_send(Completion {
          generation,
          input_revision,
          mutation,
          result,
        })
        .unwrap()
    });
    cx.run_until_parked();
  };
  let skill = agent_protocol::v1::Skill {
    name: "obsolete-skill".into(),
    content_sha256: "old-hash".into(),
    ..Default::default()
  };
  deliver(old, 0, false, Ok(Reply::Skills(vec![skill.clone()])), cx);
  deliver(
    old,
    0,
    false,
    Err(anyhow::anyhow!("old credential scope error")),
    cx,
  );
  deliver(
    old,
    0,
    true,
    Ok(Reply::Changed("old successful write".into())),
    cx,
  );
  cx.update(|cx| {
    let view = pane.read(cx);
    assert!(view.skills.is_empty());
    assert!(view.error.is_none());
    assert!(view.notice.is_none());
    assert_eq!(
      view.pending_mutations, 0,
      "stale writes still release their connection guard"
    );
  });
  let current = cx.update(|cx| pane.read(cx).generation);
  // A source edited and restored to its old value still revokes inspection authorization.
  cx.update(|cx| pane.update(cx, |pane, _| pane.input_revision += 2));
  deliver(
    current,
    0,
    false,
    Ok(Reply::InspectedSkill(
      String::new(),
      "daemon_user".into(),
      agent_protocol::v1::SkillInspection {
        skill: Some(skill.clone()),
        files: Vec::new(),
      },
    )),
    cx,
  );
  cx.update(|cx| assert!(pane.read(cx).skill_receipt.is_none()));
  // Catalog reads are independent of form edits and must still reach the view.
  deliver(current, 0, false, Ok(Reply::Skills(vec![skill])), cx);
  cx.update(|cx| assert_eq!(pane.read(cx).skills.len(), 1));
}
