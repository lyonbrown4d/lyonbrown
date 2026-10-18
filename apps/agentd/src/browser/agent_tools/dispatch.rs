use super::{BrowserTool, input::Operation};
use crate::browser::bridge::SpaceBinding;
use agent_browser::Reply;
use agent_runtime::RuntimeError;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

impl Operation {
  pub(super) async fn dispatch(
    &self,
    tool: &BrowserTool,
    host: &str,
    exact: &Value,
    approval_id: &str,
    cancel: &tokio_util::sync::CancellationToken,
  ) -> Result<Value> {
    {
      let routes = tool.daemon.browser.routes.lock().await;
      ensure!(
        !matches!(self, Self::OpenSpace { .. }) || routes.spaces.len() < 128,
        "browser space limit reached"
      );
      ensure!(
        !matches!(self, Self::OpenTab { .. }) || routes.tabs.len() < 512,
        "browser tab limit reached"
      );
    }
    ensure!(
      self.host(tool).await? == host,
      "browser runtime binding changed"
    );
    tool.fence(cancel)?;
    // The command and exact approval originate from this same parsed operation.
    // Capture both before committing; dispatch cannot substitute a later command.
    ensure!(exact["host_id"] == host, "browser approval host changed");
    let command = self.command(&tool.session_id);
    let proof =
      tool
        .daemon
        .store
        .begin_approved_browser_action(agent_storage::ApprovedBrowserAction {
          run_id: &tool.run_id,
          owner_epoch: tool.epoch,
          action_id: self.id(),
          tool: tool.name,
          arguments: exact,
          summary: crate::browser::mutation::audit_summary(&command),
          approval_id,
        })?;
    // No await between the commit and guard construction: dropping this future
    // after authorization always leaves a durable receipt or an Uncertain intent.
    let mutation = crate::browser::mutation::CommittedMutation::approved(
      tool.daemon.store.clone(),
      proof,
      command,
    );
    tool.daemon.events.notify_waiters();
    let reply = mutation.execute(&tool.daemon).await.map_err(|error| {
      if error.code() == tonic::Code::Unknown {
        anyhow::Error::new(RuntimeError::Uncertain(error.message().into()))
      } else {
        anyhow::Error::new(error)
      }
    })?;
    match (self, reply) {
      (Self::Act(action), Reply::Snapshot(snapshot)) => {
        let observation = tool
          .daemon
          .browser_snapshot_view(snapshot, &action.tab_id)
          .await
          .map_err(|error| {
            RuntimeError::Uncertain(format!(
              "browser acted but observation failed: {}",
              error.message()
            ))
          })?;
        Ok(
          json!({"action_id":self.id(),"status":"completed","untrusted_page_content":true,
          "observation":{"tab_id":observation.tab_id,"snapshot_id":observation.snapshot_id,
          "revision":observation.revision,"accessibility_json":observation.accessibility_json,
          "screenshot_artifact":observation.screenshot_artifact}}),
        )
      }
      (Self::OpenSpace { .. }, Reply::Space(space)) => {
        let mut routes = tool.daemon.browser.routes.lock().await;
        if routes.spaces.contains_key(&space.id) {
          return Err(
            RuntimeError::Uncertain("runtime reused browser space identity".into()).into(),
          );
        }
        let response = json!({"action_id":self.id(),"space_id":space.id,
          "host_id":host,"profile_id":space.profile_id});
        routes.spaces.insert(
          space.id.clone(),
          SpaceBinding {
            space,
            host: host.into(),
          },
        );
        Ok(response)
      }
      (Self::OpenTab { .. }, Reply::Tab(tab)) => {
        let mut routes = tool.daemon.browser.routes.lock().await;
        if routes.tabs.contains_key(&tab.id) {
          return Err(RuntimeError::Uncertain("runtime reused browser tab identity".into()).into());
        }
        let response = json!({"action_id":self.id(),"tab_id":tab.id,"space_id":tab.space_id});
        routes.tabs.insert(tab.id.clone(), tab);
        Ok(response)
      }
      (Self::CloseSpace { space, .. }, Reply::Done) => {
        let mut routes = tool.daemon.browser.routes.lock().await;
        routes.spaces.remove(space);
        routes.tabs.retain(|_, tab| &tab.space_id != space);
        Ok(json!({"action_id":self.id(),"status":"completed"}))
      }
      _ => Err(RuntimeError::Uncertain("browser operation response mismatch".into()).into()),
    }
  }
}
