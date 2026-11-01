use super::state::ResourcePane;
use gpui_kit::{TestSupportExt as _, *};

impl ResourcePane {
  pub(super) fn status_view(&self, _: &mut Context<Self>) -> AnyElement {
    let mut page = div().flex().flex_col().gap_3().child("Runtime status");
    if let Some(status) = &self.status {
      page = page
        .child(
          div()
            .id("runtime-model")
            .test_support()
            .aria_label(status.model.clone())
            .child(format!(
              "Provider: {} · Model: {}",
              status.provider, status.model
            )),
        )
        .child(format!(
          "Pending approvals: {} · Tools: {} · MCP servers: {}",
          status.pending_approvals, status.tools, status.mcp_servers
        ));
      if let Some(node) = &status.node {
        page = page.child(format!(
          "Node: {} · Version: {} · Protocol: {}–{}",
          node.id, node.daemon_version, node.protocol_min, node.protocol_max
        ));
      }
      if let Some(session) = &status.session {
        page = page
          .child(
            div()
              .id("runtime-session")
              .test_support()
              .aria_label(session.id.clone())
              .child(format!(
                "Session: {} · {} · Owner epoch: {}",
                session.title, session.id, session.owner_epoch
              )),
          )
          .child(format!("Workspace: {}", session.workspace));
      }
      page = page.child(status.active_run.as_ref().map_or_else(
        || "No active run".into(),
        |run| format!("Active run: {} · {}", run.id, run.status),
      ));
    } else {
      page = page.child("Refresh to read the selected session's daemon status.");
    }
    page.into_any_element()
  }
}
