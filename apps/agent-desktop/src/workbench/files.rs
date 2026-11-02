use super::state::{Reply, Workbench};
use gpui_kit::*;

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
  pub(super) fn open_file(&mut self, path: String, cx: &mut Context<Self>) {
    if self.busy || self.pending_save || self.connection_pending {
      return;
    }
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.restored_editor_draft = false;
    self.busy = true;
    self.error = None;
    self.request(async move { Ok(Reply::File(client.read_file(&session, &path).await?)) });
    cx.notify();
  }
  pub(super) fn save_file(&mut self, cx: &mut Context<Self>) {
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
  pub(super) fn load_diff(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.busy = true;
    self.error = None;
    self.request(async move { Ok(Reply::Patch(client.workspace_diff(&session, "").await?)) });
    cx.notify();
  }
}
