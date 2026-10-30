//! Preserve unsaved editor text while the visible chat workspace changes.
use super::Workbench;
use agent_protocol::v1::WorkspaceFile;
use gpui_kit::*;

#[derive(Clone)]
pub(super) struct EditorDraft {
  pub(super) session_id: Option<String>,
  pub(super) file: WorkspaceFile,
  pub(super) content: String,
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
      || self
        .unbound_editor_drafts
        .iter()
        .any(|draft| draft.content != draft.file.content)
      || self
        .unbound_editor
        .as_ref()
        .is_some_and(|draft| draft.content != draft.file.content)
  }

  pub(super) fn stash_editor(&mut self, cx: &App) {
    if let (Some(session), Some(file)) = (&self.session, &self.file) {
      self.editor_drafts.insert(
        session.clone(),
        EditorDraft {
          session_id: Some(session.clone()),
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
    self.unbound_editor = None;
    self.restored_editor_draft = draft.is_some();
    if let (Some(client), Some(session), Some(file)) = (&self.client, &self.session, &self.file) {
      let client = client.clone();
      let session = session.clone();
      let path = file.path.clone();
      self.request(async move {
        let updated = client.read_file(&session, &path).await?;
        Ok(super::state::Reply::Revision(
          updated.path,
          updated.revision,
        ))
      });
    }
    self.editor.update(cx, |editor, cx| {
      if let Some(draft) = draft {
        editor.set_highlighter(language(&draft.file.path), cx);
        editor.set_value(draft.content, window, cx);
      } else {
        editor.set_value("", window, cx);
      }
    });
  }

  pub(crate) fn persisted_editor_drafts(
    &self,
    cx: &App,
  ) -> Vec<crate::local_state::EditorDraftState> {
    let mut drafts = self
      .editor_drafts
      .iter()
      .filter(|(_, draft)| draft.content != draft.file.content)
      .map(|(session_id, draft)| snapshot(Some(session_id.clone()), draft))
      .collect::<Vec<_>>();
    drafts.extend(
      self
        .unbound_editor_drafts
        .iter()
        .filter(|draft| draft.content != draft.file.content)
        .map(|draft| snapshot(draft.session_id.clone(), draft)),
    );
    if let Some(draft) = &self.unbound_editor {
      let text = self.editor.read(cx).value().to_string();
      if text != draft.content {
        let mut current = snapshot(draft.session_id.clone(), draft);
        current.text = text;
        drafts
          .retain(|saved| !(saved.session_id == current.session_id && saved.path == current.path));
        drafts.push(current);
      }
    }
    if let (Some(session_id), Some(file)) = (&self.session, &self.file) {
      let content = self.editor.read(cx).value().to_string();
      if content != file.content {
        drafts.push(crate::local_state::EditorDraftState {
          session_id: Some(session_id.clone()),
          path: file.path.clone(),
          base_revision: file.revision.clone(),
          base_content: file.content.clone(),
          text: content,
        });
      }
    }
    drafts
  }

  pub(crate) fn restore_editor_drafts(
    &mut self,
    drafts: Vec<crate::local_state::EditorDraftState>,
    sessions: &std::collections::HashSet<String>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self.editor_drafts.clear();
    self.unbound_editor_drafts.clear();
    self.unbound_editor = None;
    for draft in drafts {
      let session_id = draft.session_id.clone();
      let editor_draft = EditorDraft {
        session_id: session_id.clone(),
        file: WorkspaceFile {
          path: draft.path,
          content: draft.base_content,
          revision: draft.base_revision,
        },
        content: draft.text,
      };
      if let Some(session_id) = session_id.filter(|id| sessions.contains(id)) {
        self.editor_drafts.insert(session_id, editor_draft);
      } else {
        self.unbound_editor_drafts.push(editor_draft);
      }
    }
    if let Some(draft) = self.unbound_editor_drafts.first().cloned() {
      self.open_unbound_editor(draft, window, cx);
    }
    if self
      .session
      .as_ref()
      .is_some_and(|session| self.editor_drafts.contains_key(session))
    {
      self.restore_editor(window, cx);
    }
  }

  pub(crate) fn refresh_after_reconnect(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
    let current_file = self.file.as_ref().map(|file| file.path.clone());
    if let (Some(client), Some(session), Some(path)) =
      (self.client.clone(), self.session.clone(), current_file)
    {
      self.request(async move {
        let current = client.read_file(&session, &path).await?;
        Ok(super::state::Reply::Revision(
          current.path,
          current.revision,
        ))
      });
    }
    self.watch_workspace();
    if self.session.is_some() {
      self.load_directory(self.directory.clone(), cx);
    }
  }

  pub(super) fn open_unbound_editor(
    &mut self,
    draft: EditorDraft,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self.file = None;
    self.unbound_editor = Some(draft.clone());
    self.restored_editor_draft = true;
    self.editor.update(cx, |editor, cx| {
      editor.set_highlighter(language(&draft.file.path), cx);
      editor.set_value(draft.content, window, cx);
    });
    self.error = Some(format!(
      "Unbound restored draft: {}. It is not attached to a session and cannot be saved until you bind it explicitly.",
      draft.file.path
    ));
    cx.notify();
  }

  pub(super) fn bind_unbound_editor(&mut self, cx: &mut Context<Self>) {
    let (Some(session), Some(draft), Some(client)) = (
      self.session.clone(),
      self.unbound_editor.clone(),
      self.client.clone(),
    ) else {
      return;
    };
    let path = draft.file.path.clone();
    self.file = Some(draft.file.clone());
    self.unbound_editor = None;
    self
      .unbound_editor_drafts
      .retain(|saved| saved.file.path != draft.file.path);
    self.restored_editor_draft = true;
    self.error = None;
    self.busy = true;
    self.editor_drafts.insert(
      session.clone(),
      EditorDraft {
        session_id: Some(session.clone()),
        ..draft
      },
    );
    self.request(async move {
      let current = client.read_file(&session, &path).await?;
      Ok(super::state::Reply::Revision(
        current.path,
        current.revision,
      ))
    });
    cx.notify();
  }
}

fn snapshot(
  session_id: Option<String>,
  draft: &EditorDraft,
) -> crate::local_state::EditorDraftState {
  crate::local_state::EditorDraftState {
    session_id,
    path: draft.file.path.clone(),
    base_revision: draft.file.revision.clone(),
    base_content: draft.file.content.clone(),
    text: draft.content.clone(),
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
