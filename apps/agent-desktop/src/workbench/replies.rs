use super::state::{Reply, Workbench};
use gpui_kit::component::diff::DiffFile;
use gpui_kit::*;
impl Workbench {
  pub(super) fn apply(&mut self, reply: Reply, window: &mut Window, cx: &mut Context<Self>) {
    // A same-tab frame read may complete after takeover. Its pixels, empty
    // result, or error cannot update the new control generation.
    if matches!(&reply,
      Reply::Frame(generation, ..) | Reply::NoFrame(generation) | Reply::FrameError(generation, _)
        if *generation != self.frame_generation)
    {
      return;
    }
    if matches!(reply, Reply::Saved(_) | Reply::SaveError(_)) {
      self.pending_save = false;
    }
    if matches!(
      reply,
      Reply::Attached(_)
        | Reply::Tab(..)
        | Reply::TabOpenFailed(..)
        | Reply::Closed
        | Reply::Control(_)
        | Reply::BrowserOperationError(_)
    ) {
      self.pending_browser_operation = false;
    }
    match reply {
      Reply::Attached(attachment) => {
        self.attachment = Some(attachment);
        self.busy = false;
        self.open_browser(cx);
      }
      Reply::WatchError(error) => self.error = Some(error),
      Reply::Revision(path, revision) => {
        if self.error.is_none()
          && self
            .file
            .as_ref()
            .is_some_and(|file| file.path == path && file.revision != revision)
        {
          self.error=Some("File changed in the workspace. Reload to inspect it; Save will validate the original revision.".into());
        }
      }
      Reply::Changed(paths) => {
        if !self.busy {
          if let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) {
            let directory = self.directory.clone();
            self.request(async move {
              Ok(match client.list_files(&session, &directory).await {
                Ok(files) => Reply::BackgroundFiles(directory, files),
                Err(error) => Reply::WatchError(error.to_string()),
              })
            });
          }
          if let (Some(client), Some(session), Some(file)) =
            (self.client.clone(), self.session.clone(), self.file.clone())
            && paths.contains(&file.path)
          {
            self.request(async move {
              let updated = client.read_file(&session, &file.path).await?;
              Ok(Reply::Revision(updated.path, updated.revision))
            });
          }
        }
      }
      Reply::BackgroundFiles(directory, entries) => {
        self.directory = directory;
        self.files = entries;
      }
      Reply::Files(directory, entries) => {
        self.directory = directory;
        self.files = entries;
        self.busy = false;
      }
      Reply::File(file) | Reply::Saved(file) => {
        let language = super::drafts::language(&file.path);
        self.editor.update(cx, |editor, cx| {
          editor.set_highlighter(language, cx);
          editor.set_value(file.content.clone(), window, cx);
        });
        self.file = Some(file);
        self.busy = false;
        self.error = None;
      }
      Reply::Patch(patch) => {
        match DiffFile::parse(&patch) {
          Ok(files) => {
            self.diff.update(cx, |diff, cx| diff.set_files(files, cx));
            self.patch = patch;
          }
          Err(error) => self.error = Some(format!("Cannot render daemon patch: {error}")),
        }
        self.busy = false;
      }
      Reply::Tab(space, tab) => {
        self.input_queue.clear();
        self.input_frame_pending = false;
        self.space = Some(space);
        self.tab = Some(tab);
        self.requested_viewport = None;
        self.invalidate_frames();
        self.busy = false;
      }
      Reply::TabOpenFailed(space, error) => {
        self.space = Some(space);
        self.busy = false;
        self.browser_error = Some(error);
        self.input_queue.clear();
      }
      Reply::Closed => {
        if let Some(session) = &self.session {
          self.browser_sessions.remove(session);
        }
        self.space = None;
        self.tab = None;
        self.invalidate_frames();
        self.busy = false;
        self.browser_error = None;
        self.input_queue.clear();
      }
      Reply::Control(space) => {
        self.input_queue.clear();
        self.invalidate_frames();
        self.space = Some(space);
        self.busy = false;
        self.fetch_frame(cx);
      }
      Reply::Frame(_, frame, refresh_input) => {
        if self.tab.as_ref().is_none_or(|tab| tab.id != frame.tab_id) {
          return;
        }
        self.frame_pending = false;
        self.accept_frame(frame);
        if refresh_input {
          self.input_frame_pending = false;
          self.send_next_input();
        }
      }
      Reply::NoFrame(_) => self.frame_pending = false,
      Reply::Input => {
        self.input_pending = false;
        self.input_frame_pending = true;
        self.fetch_frame(cx);
      }
      Reply::InputError(error) => {
        self.input_pending = false;
        self.input_frame_pending = false;
        self.input_queue.clear();
        self.browser_error = Some(error);
      }
      Reply::BrowserOperationError(error) | Reply::FrameError(_, error) => {
        self.frame_pending = false;
        self.busy = false;
        self.browser_error = Some(error);
        self.input_queue.clear();
      }
      Reply::Error(error) | Reply::SaveError(error) => {
        self.error = Some(error);
        self.busy = false;
      }
    }
    cx.notify();
  }
}
