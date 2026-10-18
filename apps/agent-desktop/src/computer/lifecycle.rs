use super::ComputerPane;
use agent_client::AgentClient;
use gpui_kit::*;

impl ComputerPane {
  pub(crate) fn bind(
    &mut self,
    client: Option<AgentClient>,
    session: Option<String>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if self.session != session || self.client.is_some() != client.is_some() {
      self.invalidate(window, cx);
      self.session = session;
    }
    self.client = client;
    cx.notify();
  }

  /// An explicit dial attempt revokes the old host even if authentication fails.
  /// Clearing the catalog also makes selected_target return None immediately;
  /// the select widget is cleared on the next window-bound reset or bind.
  pub(crate) fn begin_connection(&mut self, cx: &mut Context<Self>) {
    self.invalidate_operation();
    self.revoke_host();
    self.connection_pending = true;
    cx.notify();
  }

  pub(crate) fn reset_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.invalidate(window, cx);
    self.revoke_host();
    cx.notify();
  }

  fn revoke_host(&mut self) {
    self.attachment = None;
    self.bound_session = None;
    self.bound_scope = None;
    self.client = None;
    self.session = None;
    self.error = None;
    self.detach_warning = false;
  }

  fn invalidate_operation(&mut self) {
    self.generation = self.generation.saturating_add(1);
    if let Some(operation) = self.operation.take() {
      operation.abort();
    }
    self.pending = false;
    self.targets.clear();
  }

  pub(super) fn invalidate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.invalidate_operation();
    self.select.update(cx, |select, cx| {
      select.set_selected_index(None, window, cx);
      select.set_items(Vec::new(), window, cx);
    });
  }
}

impl Drop for ComputerPane {
  fn drop(&mut self) {
    if let Some(operation) = self.operation.take() {
      operation.abort();
    }
    // RuntimeAttachment::drop revokes this host; daemon jobs are independent.
  }
}
