use super::MemoryPanel;
use agent_protocol::v1;
use gpui_kit::{
  component::{
    ActiveTheme, Disableable, Sizable,
    button::Button,
    input::{Input, InputState},
  },
  *,
};
impl Render for MemoryPanel {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if self.reset_inputs {
      self.content = cx.new(|cx| InputState::new(window, cx).placeholder("Memory content"));
      self.query = cx.new(|cx| InputState::new(window, cx).placeholder("Search active memories"));
      self.reset_inputs = false;
    }
    let blocked = self.busy || self.pending || self.client.is_none();
    let state = self
      .selected
      .as_ref()
      .and_then(|item| v1::MemoryState::try_from(item.state).ok());
    let editable = !matches!(state, Some(v1::MemoryState::Deleted));
    let activatable = matches!(
      state,
      None | Some(v1::MemoryState::Proposed | v1::MemoryState::Active)
    );
    let mut page = div().flex().flex_col().gap_3()
      .child(if self.session.is_some() { "User, project and selected session memories" } else { "User memories · select a chat to include its project and session" })
      .child("Model proposals remain inactive until you choose Activate. Memory text cannot approve tools or expand permissions.")
      .child(div().flex().flex_wrap().gap_2()
        .child(Button::new("memory-refresh").small().label(if self.busy {"Loading…"} else {"Refresh"}).disabled(blocked).on_click(cx.listener(|this, _, _, cx| this.refresh(cx))))
        .child(Button::new("memory-new").small().label("New memory").disabled(blocked).on_click(cx.listener(|this, _, window, cx| this.fresh(window, cx)))))
      .child(Input::new(&self.query).id("memory-search-query").disabled(blocked))
      .child(Button::new("memory-search").small().label("Search active memories").disabled(blocked).on_click(cx.listener(|this, _, _, cx| this.search(cx))));
    if self.items.is_empty() && !self.busy {
      page =
        page.child("No memories in this result. Create one, or refresh to leave search results.");
    }
    page = page.children(self.items.iter().enumerate().map(|(index, item)| {
      let item = item.clone();
      let selected = self
        .selected
        .as_ref()
        .is_some_and(|current| current.id == item.id);
      let scope = item
        .scope
        .as_ref()
        .and_then(|scope| v1::MemoryScopeKind::try_from(scope.kind).ok());
      Button::new(("memory-item", index))
        .small()
        .w_full()
        .label(format!(
          "{}{:?} · {:?} · revision {} · {}",
          if selected { "Selected · " } else { "" },
          scope.unwrap_or_default(),
          v1::MemoryState::try_from(item.state).unwrap_or_default(),
          item.revision,
          item.content.chars().take(120).collect::<String>()
        ))
        .disabled(blocked)
        .on_click(cx.listener(move |this, _, window, cx| this.select(item.clone(), window, cx)))
    }));
    if !self.next_cursor.is_empty() {
      page = page.child(
        Button::new("memory-more")
          .small()
          .label("Load more")
          .disabled(blocked)
          .on_click(cx.listener(|this, _, _, cx| this.more(cx))),
      );
    }
    if self.selected.is_none() {
      page = page.child(
        div()
          .flex()
          .flex_wrap()
          .items_center()
          .gap_2()
          .child("Scope")
          .children(
            [
              v1::MemoryScopeKind::User,
              v1::MemoryScopeKind::Project,
              v1::MemoryScopeKind::Session,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, scope)| {
              Button::new(("memory-draft-scope", index))
                .small()
                .label(format!(
                  "{:?}{}",
                  scope,
                  if self.draft_scope == scope {
                    " · selected"
                  } else {
                    ""
                  }
                ))
                .disabled(blocked || (scope != v1::MemoryScopeKind::User && self.session.is_none()))
                .on_click(cx.listener(move |this, _, _, cx| {
                  this.draft_scope = scope;
                  cx.notify();
                }))
            }),
          ),
      );
      page = page.child(
        div()
          .flex()
          .flex_wrap()
          .items_center()
          .gap_2()
          .child("Kind")
          .children(
            [
              v1::MemoryKind::Preference,
              v1::MemoryKind::Fact,
              v1::MemoryKind::Procedure,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
              Button::new(("memory-draft-kind", index))
                .small()
                .label(format!(
                  "{:?}{}",
                  kind,
                  if self.draft_kind == kind {
                    " · selected"
                  } else {
                    ""
                  }
                ))
                .disabled(blocked)
                .on_click(cx.listener(move |this, _, _, cx| {
                  this.draft_kind = kind;
                  cx.notify();
                }))
            }),
          ),
      );
    }
    page = page
      .child(
        div()
          .mt_4()
          .font_weight(FontWeight::SEMIBOLD)
          .child(if self.selected.is_some() {
            "Selected memory"
          } else {
            "New memory"
          }),
      )
      .child(
        Input::new(&self.content)
          .id("memory-content")
          .disabled(blocked || !editable),
      )
      .child(
        div()
          .flex()
          .flex_wrap()
          .gap_2()
          .child(
            Button::new("memory-save")
              .small()
              .label(if state == Some(v1::MemoryState::Proposed) {
                "Activate"
              } else {
                "Save as active"
              })
              .disabled(blocked || !activatable)
              .on_click(cx.listener(|this, _, _, cx| this.save(v1::MemoryState::Active, cx))),
          )
          .child(
            Button::new("memory-reject")
              .small()
              .label("Reject proposal")
              .disabled(blocked || state != Some(v1::MemoryState::Proposed))
              .on_click(cx.listener(|this, _, _, cx| this.save(v1::MemoryState::Rejected, cx))),
          )
          .child(
            Button::new("memory-forget")
              .small()
              .label("Forget permanently")
              .disabled(blocked || self.selected.is_none() || !editable)
              .on_click(cx.listener(|this, _, _, cx| this.forget(cx))),
          ),
      );
    for (index, job) in self.jobs.iter().enumerate() {
      let job = job.clone();
      let retry = matches!(
        v1::MemoryJobState::try_from(job.state),
        Ok(v1::MemoryJobState::Failed | v1::MemoryJobState::Uncertain)
      );
      page = page.child(
        div()
          .flex()
          .flex_wrap()
          .items_center()
          .gap_2()
          .child(format!(
            "Index job · {:?} · attempt {}",
            v1::MemoryJobState::try_from(job.state).unwrap_or_default(),
            job.attempts
          ))
          .child(
            Button::new(("memory-retry", index))
              .small()
              .label("Retry job")
              .disabled(blocked || !retry)
              .on_click(cx.listener(move |this, _, _, cx| this.retry(job.clone(), cx))),
          ),
      );
    }
    let ready = self
      .status
      .as_ref()
      .is_some_and(|status| status.retrieval_ready);
    page = page.child(div().mt_4().font_weight(FontWeight::SEMIBOLD).child("Retrieval index"))
      .child(self.status.as_ref().map_or_else(|| "Refresh to read index status.".into(), |status| format!("{} · {} · {} dimensions", if status.retrieval_ready {"Ready"} else {"Unavailable"}, status.embedding_model, status.dimensions)))
      .child(Button::new("memory-rebuild").small().label("Rebuild current scopes").disabled(blocked || !ready).on_click(cx.listener(|this, _, _, cx| this.rebuild(cx))))
      .child("Rebuilds scan in batches. Completion includes their index jobs; interrupted jobs require an explicit retry.")
      .children(self.rebuilds.iter().take(5).map(|rebuild| div().child(format!("{:?} · {} scanned · {} index jobs · {}", v1::MemoryRebuildState::try_from(rebuild.state).unwrap_or_default(), rebuild.scanned, rebuild.enqueued, rebuild.embedding_model))))
      .child(div().text_color(cx.theme().muted_foreground).child(self.message.clone()));
    page
  }
}
