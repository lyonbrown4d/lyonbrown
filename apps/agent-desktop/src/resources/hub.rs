use super::state::{Reply, ResourcePane};
use agent_protocol::v1::HubRequest;
use gpui_kit::*;
use serde_json::Value;

pub(super) fn text(value: &Value, key: &str) -> String {
  value
    .get(key)
    .and_then(Value::as_str)
    .unwrap_or_default()
    .to_string()
}
impl ResourcePane {
  fn hub_input(&self, action: &str, cx: &App) -> HubRequest {
    HubRequest {
      action: action.into(),
      repository: self.repository.read(cx).value().to_string(),
      revision: self.revision.read(cx).value().to_string(),
      file: self.file.read(cx).value().to_string(),
      runtime: self
        .backend
        .read(cx)
        .selected_value()
        .cloned()
        .unwrap_or_default(),
      id: String::new(),
    }
  }
  pub(super) fn same_source(&self, input: &HubRequest, cx: &App) -> bool {
    let current = self.hub_input("", cx);
    current.repository == input.repository && current.revision == input.revision
  }
  pub(super) fn current_plan(&self, cx: &App) -> Option<&(HubRequest, Value)> {
    let current = self.hub_input("", cx);
    self.hub_plan.as_ref().filter(|(input, _)| {
      self.same_source(input, cx) && current.file == input.file && current.runtime == input.runtime
    })
  }
  pub(super) fn hub_action(&mut self, action: &str, id: String, cx: &mut Context<Self>) {
    if matches!(action, "load" | "unload" | "remove")
      && !self
        .installed
        .iter()
        .any(|model| model.get("plan").is_some_and(|plan| text(plan, "id") == id))
    {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    let mut request = self.hub_input(action, cx);
    request.id = id;
    if action == "search" {
      request.repository = self.query.read(cx).value().to_string();
    }
    if matches!(action, "plan" | "install") {
      let Some((inspection_request, inspected)) = &self.hub_inspection else {
        return;
      };
      if !self.same_source(inspection_request, cx) {
        return;
      }
      // Use the immutable revision agentd inspected when asking for a plan.
      let revision = text(inspected, "revision");
      if revision.is_empty() {
        return;
      }
      request.revision = revision;
    }
    if action == "install" {
      let Some((plan_request, plan)) = self.current_plan(cx) else {
        return;
      };
      request = plan_request.clone();
      request.revision = text(plan, "revision");
      request.action = "install".into();
      request.id = text(plan, "id");
      if request.id.is_empty() {
        return;
      }
      self.hub_plan = None;
    }
    if action == "inspect" {
      self.hub_inspection = None;
      self.hub_plan = None;
    }
    if action == "plan" {
      self.hub_plan = None;
    }
    let mutation = matches!(action, "install" | "cancel" | "load" | "unload" | "remove");
    self.request(
      mutation,
      async move {
        let json = client.hub_operation(request.clone()).await?;
        Ok(Reply::Hub(request, serde_json::from_str(&json)?))
      },
      cx,
    );
  }
  pub(super) fn apply_hub(&mut self, request: HubRequest, value: Value, cx: &mut Context<Self>) {
    match request.action.as_str() {
      "search" => self.hub_results = value.as_array().cloned().unwrap_or_default(),
      "inspect" => {
        if self.same_source(&request, cx) {
          self.hub_inspection = Some((request, value));
        }
      }
      "plan" => {
        // The request is pinned to the inspected revision; compare current editable source separately.
        if let Some((source, _)) = &self.hub_inspection
          && self.same_source(source, cx)
          && request.file == self.file.read(cx).value().as_ref()
          && Some(&request.runtime) == self.backend.read(cx).selected_value()
        {
          let mut editable = request;
          editable.revision = source.revision.clone();
          self.hub_plan = Some((editable, value));
        }
      }
      "installed" => self.installed = value.as_array().cloned().unwrap_or_default(),
      "install" | "status" | "cancel" => {
        let id = text(&value, "id");
        if !id.is_empty() {
          self.jobs.insert(id, value);
        }
      }
      "remove" => {
        self.installed.retain(|model| {
          model
            .get("plan")
            .is_none_or(|plan| text(plan, "id") != request.id)
        });
        self.refresh(cx);
        self.notice = Some("Installed artifact removed".into());
      }
      _ => {
        self.refresh(cx);
        self.notice = Some(format!(
          "{} completed; refresh installed models to reconcile artifacts",
          request.action
        ));
      }
    }
  }
}
