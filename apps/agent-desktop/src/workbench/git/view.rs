use super::GitPane;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, button::Button, diff::Diff};
use gpui_kit::{TestSupportExt as _, prelude::FluentBuilder as _, *};

impl Render for GitPane {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .id("git-panel")
      .test_support()
      .size_full()
      .flex()
      .flex_col()
      .child(
        div()
          .p_2()
          .flex()
          .gap_2()
          .child(
            Button::new("git-refresh")
              .small()
              .label("Refresh Git")
              .disabled(self.busy || self.session.is_none())
              .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
          )
          .child(
            Button::new("git-patch")
              .small()
              .label("All changes")
              .disabled(self.busy || !self.status.as_ref().is_some_and(|status| status.repository))
              .on_click(cx.listener(|this, _, _, cx| this.patch(String::new(), cx))),
          ),
      )
      .child(
        div()
          .id("git-summary")
          .test_support()
          .px_3()
          .text_xs()
          .child(
            self
              .status
              .as_ref()
              .map(|status| {
                if status.repository {
                  format!(
                    "{} · {} · ↑{} ↓{} · {}",
                    status.branch,
                    status.head.chars().take(8).collect::<String>(),
                    status.ahead,
                    status.behind,
                    status.upstream
                  )
                } else {
                  "Workspace is not a Git repository".into()
                }
              })
              .unwrap_or_else(|| "Select a session and refresh Git status".into()),
          ),
      )
      .when_some(self.error.clone(), |view, error| {
        view.child(
          div()
            .id("git-error")
            .test_support()
            .p_2()
            .text_color(cx.theme().danger)
            .child(error),
        )
      })
      .child(
        div()
          .id("git-files")
          .test_support()
          .flex()
          .flex_col()
          .max_h(px(220.))
          .overflow_y_scroll()
          .children(
            self
              .status
              .iter()
              .flat_map(|status| status.entries.iter())
              .enumerate()
              .map(|(index, entry)| {
                let path = entry.path.clone();
                let untracked = entry.index_status == "?" || entry.worktree_status == "?";
                div()
                  .flex()
                  .flex_col()
                  .items_start()
                  .gap_1()
                  .child(
                    Button::new(("git-file", index))
                      .small()
                      .label(format!(
                        "{}{} {}{}",
                        entry.index_status,
                        entry.worktree_status,
                        entry.path,
                        if entry.original_path.is_empty() {
                          String::new()
                        } else {
                          format!(" ← {}", entry.original_path)
                        }
                      ))
                      .disabled(self.busy || untracked)
                      .on_click(cx.listener(move |this, _, _, cx| this.patch(path.clone(), cx))),
                  )
                  .when(untracked, |row| {
                    row.child(
                      div()
                        .id(("git-untracked", index))
                        .test_support()
                        .aria_label("Untracked preview unavailable; open this file in Files")
                        .text_xs()
                        .child("Untracked preview unavailable · open this file in Files"),
                    )
                  })
              }),
          ),
      )
      .child(div().px_3().text_xs().child(if self.path.is_empty() {
        "Workspace patch".to_string()
      } else {
        self.path.clone()
      }))
      .child(
        div()
          .flex_1()
          .min_h_0()
          .child(Diff::new(&self.diff).size_full()),
      )
  }
}
