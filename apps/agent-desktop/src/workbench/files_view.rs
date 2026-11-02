use super::files::SaveFile;
use super::state::Workbench;
use gpui_kit::component::{
  ActiveTheme, Disableable, Sizable,
  button::Button,
  diff::{Diff, DiffMode},
  input::Editor,
};
use gpui_kit::{TestSupportExt as _, prelude::FluentBuilder as _, *};

impl Workbench {
  fn error_view(&self, cx: &App) -> AnyElement {
    div()
      .id("workbench-error")
      .test_support()
      .text_xs()
      .p_2()
      .text_color(cx.theme().danger)
      .child(self.error.clone().unwrap_or_default())
      .into_any_element()
  }
  pub(super) fn files_view(&self, cx: &mut Context<Self>) -> AnyElement {
    let parent = std::path::Path::new(&self.directory)
      .parent()
      .map(|path| path.to_string_lossy().into_owned())
      .unwrap_or_default();
    div()
      .id("files-panel")
      .test_support()
      .flex()
      .flex_col()
      .size_full()
      .child(
        div()
          .flex()
          .gap_2()
          .p_2()
          .child(
            Button::new("files-up")
              .small()
              .label("Up")
              .disabled(self.busy || self.directory.is_empty())
              .on_click(cx.listener(move |this, _, _, cx| this.load_directory(parent.clone(), cx))),
          )
          .child(
            Button::new("files-refresh")
              .small()
              .label("Refresh")
              .disabled(self.busy || self.session.is_none())
              .on_click(
                cx.listener(|this, _, _, cx| this.load_directory(this.directory.clone(), cx)),
              ),
          ),
      )
      .child(div().px_3().text_xs().child(if self.session.is_none() {
        "Select a session workspace".into()
      } else {
        format!("/{}", self.directory)
      }))
      .child(self.error_view(cx))
      .child(
        div()
          .id("workspace-files")
          .test_support()
          .flex_1()
          .min_h_0()
          .overflow_y_scroll()
          .children(self.files.iter().enumerate().map(|(index, file)| {
            let path = file.path.clone();
            let directory = file.directory;
            div()
              .id(("workspace-file", index))
              .test_support()
              .role(Role::Button)
              .aria_label(format!(
                "Open {} {}",
                if directory { "directory" } else { "file" },
                file.path
              ))
              .px_3()
              .py_2()
              .cursor_pointer()
              .hover(|style| style.bg(cx.theme().muted))
              .child(format!(
                "{} {}",
                if directory { "▸" } else { "·" },
                file.path
              ))
              .on_click(cx.listener(move |this, _, _, cx| {
                if !this.busy {
                  if directory {
                    this.load_directory(path.clone(), cx)
                  } else {
                    this.open_file(path.clone(), cx)
                  }
                }
              }))
          })),
      )
      .into_any_element()
  }
  pub(super) fn editor_view(&self, cx: &mut Context<Self>) -> AnyElement {
    let dirty = self
      .file
      .as_ref()
      .is_some_and(|file| file.content != self.editor.read(cx).value().as_ref());
    div()
      .id("editor-panel")
      .test_support()
      .key_context("WorkbenchEditor")
      .on_action(cx.listener(|this, _: &SaveFile, _, cx| this.save_file(cx)))
      .flex()
      .flex_col()
      .size_full()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .p_2()
          .child(div().flex_1().text_xs().child(format!(
              "{}{}",
              self
                .file
                .as_ref()
                .map_or("Open a file in Files", |file| file.path.as_str()),
              if self.connection_pending {
                " · reconnecting"
              } else if self.restored_editor_draft {
                " · restored draft"
              } else if dirty {
                " · modified"
              } else {
                ""
              }
            )))
          .child(
            Button::new("file-reload")
              .small()
              .label("Reload")
              .disabled(
                self.connection_pending || self.pending_save || self.file.is_none() || self.busy,
              )
              .on_click(cx.listener(|this, _, _, cx| {
                if let Some(file) = &this.file {
                  this.open_file(file.path.clone(), cx)
                }
              })),
          )
          .child(
            Button::new("file-save")
              .small()
              .label("Save")
              .disabled(self.connection_pending || self.pending_save || !dirty || self.busy)
              .on_click(cx.listener(|this, _, _, cx| this.save_file(cx))),
          ),
      )
      .when(!self.unbound_editor_drafts.is_empty(), |panel| {
        panel.child(
          div()
            .id("unbound-editor-drafts")
            .test_support()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .child("Unbound restored drafts")
            .children(
              self
                .unbound_editor_drafts
                .iter()
                .enumerate()
                .map(|(index, draft)| {
                  let draft = draft.clone();
                  Button::new(("open-unbound-draft", index))
                    .small()
                    .label(draft.file.path.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                      this.open_unbound_editor(draft.clone(), window, cx)
                    }))
                }),
            )
            .when(
              self.unbound_editor.is_some() && self.session.is_some(),
              |row| {
                row.child(
                  Button::new("bind-unbound-draft")
                    .small()
                    .label("Bind to current session")
                    .on_click(cx.listener(|this, _, _, cx| this.bind_unbound_editor(cx))),
                )
              },
            ),
        )
      })
      .child(self.error_view(cx))
      .child(
        div()
          .id("workspace-editor")
          .test_support()
          .flex_1()
          .min_h_0()
          .child(
            Editor::new(&self.editor)
              .aria_label("Workspace file editor")
              .readonly(
                self.connection_pending || self.pending_save || self.busy || self.file.is_none(),
              )
              .size_full(),
          ),
      )
      .into_any_element()
  }
  pub(super) fn diff_view(&self, cx: &mut Context<Self>) -> AnyElement {
    div()
      .id("diff-panel")
      .test_support()
      .flex()
      .flex_col()
      .size_full()
      .child(
        div()
          .flex()
          .gap_2()
          .p_2()
          .child(
            Button::new("diff-refresh")
              .small()
              .label("Refresh patch")
              .disabled(self.session.is_none() || self.busy)
              .on_click(cx.listener(|this, _, _, cx| this.load_diff(cx))),
          )
          .child(
            Button::new("diff-mode")
              .small()
              .label("Split / unified")
              .on_click(cx.listener(|this, _, _, cx| {
                this.diff.update(cx, |diff, cx| {
                  diff.set_mode(
                    if diff.mode() == DiffMode::Split {
                      DiffMode::Unified
                    } else {
                      DiffMode::Split
                    },
                    cx,
                  )
                })
              })),
          ),
      )
      .child(self.error_view(cx))
      .child(
        div()
          .px_2()
          .text_xs()
          .text_color(cx.theme().muted_foreground)
          .child(if self.patch.is_empty() {
            "No tracked workspace changes loaded. Refresh patch to check the workspace."
          } else {
            "Daemon workspace patch"
          }),
      )
      .child(
        div()
          .flex_1()
          .min_h_0()
          .child(Diff::new(&self.diff).size_full()),
      )
      .into_any_element()
  }
}
