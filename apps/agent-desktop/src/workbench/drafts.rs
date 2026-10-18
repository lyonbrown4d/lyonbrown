//! Preserve unsaved editor text while the visible chat workspace changes.
use super::Workbench;
use agent_protocol::v1::WorkspaceFile;
use gpui_kit::*;

pub(super) struct EditorDraft {
  file: WorkspaceFile,
  content: String,
}

impl Workbench {
  pub(crate) fn blocks_connection(&self, cx: &App) -> bool {
    self.pending_save || self.has_unsaved_edits(cx)
  }

  pub(crate) fn has_unsaved_edits(&self, cx: &App) -> bool {
    self
      .file
      .as_ref()
      .is_some_and(|file| self.editor.read(cx).value().as_ref() != file.content)
      || self
        .editor_drafts
        .values()
        .any(|draft| draft.content != draft.file.content)
  }

  pub(super) fn stash_editor(&mut self, cx: &App) {
    if let (Some(session), Some(file)) = (&self.session, &self.file) {
      self.editor_drafts.insert(
        session.clone(),
        EditorDraft {
          file: file.clone(),
          content: self.editor.read(cx).value().to_string(),
        },
      );
    }
  }

  pub(super) fn restore_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let draft = self
      .session
      .as_ref()
      .and_then(|session| self.editor_drafts.remove(session));
    self.file = draft.as_ref().map(|draft| draft.file.clone());
    self.editor.update(cx, |editor, cx| {
      if let Some(draft) = draft {
        editor.set_highlighter(language(&draft.file.path), cx);
        editor.set_value(draft.content, window, cx);
      } else {
        editor.set_value("", window, cx);
      }
    });
  }
}

pub(super) fn language(path: &str) -> &'static str {
  match std::path::Path::new(path)
    .extension()
    .and_then(|value| value.to_str())
  {
    Some("rs") => "rust",
    Some("toml") => "toml",
    Some("md") => "markdown",
    _ => "plaintext",
  }
}
