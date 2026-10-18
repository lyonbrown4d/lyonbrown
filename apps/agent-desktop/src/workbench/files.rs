use super::state::{Reply, Workbench};
use gpui_kit::component::{
  ActiveTheme, Disableable, Sizable,
  button::Button,
  diff::{Diff, DiffMode},
  input::Editor,
};
use gpui_kit::{TestSupportExt as _, *};

actions!(workbench, [SaveFile]);

impl Workbench {
  pub(super) fn load_directory(&mut self, directory: String, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.busy = true;
    self.error = None;
    self.request(async move {
      let files = client.list_files(&session, &directory).await?;
      Ok(Reply::Files(directory, files))
    });
    cx.notify();
  }
  fn open_file(&mut self, path: String, cx: &mut Context<Self>) {
    if self.busy || self.pending_save || self.connection_pending {
      return;
    }
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.busy = true;
    self.error = None;
    self.request(async move { Ok(Reply::File(client.read_file(&session, &path).await?)) });
    cx.notify();
  }
  fn save_file(&mut self, cx: &mut Context<Self>) {
    if self.busy || self.pending_save || self.connection_pending {
      return;
    }
    let (Some(client), Some(session), Some(file)) =
      (self.client.clone(), self.session.clone(), self.file.clone())
    else {
      return;
    };
    let content = self.editor.read(cx).value().to_string();
    self.pending_save = true;
    self.busy = true;
    self.error = None;
    self.request(async move {
      Ok(
        match client
          .write_file(&session, &file.path, &content, &file.revision)
          .await
        {
          Ok(saved) => Reply::Saved(saved),
          Err(error) => Reply::SaveError(error.to_string()),
        },
      )
    });
    cx.notify();
  }
  fn load_diff(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.busy = true;
    self.error = None;
    self.request(async move { Ok(Reply::Patch(client.workspace_diff(&session, "").await?)) });
    cx.notify();
  }
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
