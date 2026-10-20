use super::state::{Reply, Workbench};
use gpui_kit::*;
use std::time::Instant;

impl Workbench {
  pub(crate) fn fetch_frame(&mut self, cx: &mut Context<Self>) {
    self.last_frame = Instant::now();
    self.resize_browser(cx);
    if let Some(error) = self
      .attachment
      .as_ref()
      .and_then(|attachment| attachment.error())
    {
      self.browser_error = Some(format!(
        "Browser runtime disconnected: {error}. Restart runtime to open a new page; prior actions will not be replayed."
      ));
      return;
    }
    if self.frame_pending || self.browser_error.is_some() {
      return;
    }
    let (Some(client), Some(tab)) = (self.client.clone(), self.tab.clone()) else {
      return;
    };
    self.frame_pending = true;
    let generation = self.frame_generation;
    let refresh_input = self.input_frame_pending;
    let sequence = if refresh_input {
      0
    } else {
      self.frame.as_ref().map_or(0, |(frame, _)| frame.sequence)
    };
    self.request(async move {
      Ok(match client.browser_frame(&tab.id, sequence).await {
        Ok(frame) => Reply::Frame(generation, frame, refresh_input),
        Err(error) if crate::connection::frame_not_ready(&error) => Reply::NoFrame(generation),
        Err(error) => Reply::FrameError(generation, error.to_string()),
      })
    });
  }

  pub(crate) fn invalidate_frames(&mut self) {
    self.frame_generation = self.frame_generation.saturating_add(1);
    self.frame = None;
    self.frame_pending = false;
  }
}
