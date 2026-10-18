//! Daemon-owned tools: page content is untrusted and mutations require exact approval.
mod dispatch;
mod input;
mod schema;
use crate::Daemon;
use agent_core::{Run, Session};
use agent_runtime::{Result, RuntimeError, RuntimeTool, ToolCall, ToolDefinition, ToolOutput};
use anyhow::ensure;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct BrowserTool {
  daemon: Daemon,
  run_id: String,
  session_id: String,
  epoch: u64,
  name: &'static str,
}

pub(crate) fn tools(daemon: Daemon, run: &Run, session: &Session) -> Vec<Arc<dyn RuntimeTool>> {
  ["browser_observe", "browser_act"]
    .into_iter()
    .map(|name| {
      Arc::new(BrowserTool {
        daemon: daemon.clone(),
        run_id: run.id.clone(),
        session_id: session.id.clone(),
        epoch: session.owner_epoch,
        name,
      }) as Arc<dyn RuntimeTool>
    })
    .collect()
}

#[async_trait::async_trait]
impl RuntimeTool for BrowserTool {
  fn definition(&self) -> ToolDefinition {
    schema::definition(self.name)
  }
  async fn execute(&self, call: ToolCall, cancel: CancellationToken) -> Result<ToolOutput> {
    match self.perform(call, cancel).await {
      Ok(value) => Ok(ToolOutput {
        text: value.to_string(),
        is_error: false,
      }),
      Err(error) => match error.downcast::<RuntimeError>() {
        Ok(error) => Err(error),
        Err(error) => Ok(ToolOutput {
          text: error.to_string(),
          is_error: true,
        }),
      },
    }
  }
}
impl BrowserTool {
  fn fence(&self, cancel: &CancellationToken) -> anyhow::Result<()> {
    ensure!(
      !cancel.is_cancelled(),
      "browser tool cancelled before dispatch"
    );
    let session = self
      .daemon
      .store
      .session(&self.session_id)?
      .ok_or_else(|| anyhow::anyhow!("browser session unavailable"))?;
    ensure!(
      session.owner_node_id == self.daemon.node_id && session.owner_epoch == self.epoch,
      "browser session ownership changed"
    );
    let run = self
      .daemon
      .store
      .run(&self.run_id)?
      .ok_or_else(|| anyhow::anyhow!("browser run unavailable"))?;
    ensure!(
      run.session_id == self.session_id
        && !matches!(
          run.status.as_str(),
          "cancel_requested" | "cancelled" | "completed" | "failed" | "uncertain"
        ),
      "browser run is no longer active"
    );
    Ok(())
  }
  async fn tab(&self, id: &str, mutate: bool) -> anyhow::Result<String> {
    let (host, session, human) = self
      .daemon
      .browser_tab_route(id)
      .await
      .map_err(|_| anyhow::anyhow!("authorized browser tab unavailable"))?;
    ensure!(
      session == self.session_id,
      "authorized browser tab unavailable"
    );
    ensure!(!mutate || !human, "human takeover is active");
    Ok(host)
  }
  async fn space(&self, id: &str) -> anyhow::Result<String> {
    let (host, session, human) = self
      .daemon
      .browser_space_route(id)
      .await
      .map_err(|_| anyhow::anyhow!("authorized browser space unavailable"))?;
    ensure!(
      session == self.session_id,
      "authorized browser space unavailable"
    );
    ensure!(!human, "human takeover is active");
    Ok(host)
  }
  async fn perform(&self, call: ToolCall, cancel: CancellationToken) -> anyhow::Result<Value> {
    self.fence(&cancel)?;
    if self.name == "browser_observe" {
      return self.observe(call.arguments, cancel).await;
    }
    let operation = input::Operation::parse(&call.arguments, &self.run_id, &call.id)?;
    let selected_host = operation.host(self).await?;
    let exact = json!({"session_id":self.session_id,"owner_epoch":self.epoch,
      "host_id":selected_host,"action_id":operation.id(),"call":call.arguments["call"]});
    crate::authorization::approve(
      &self.daemon,
      &self.run_id,
      self.epoch,
      self.name,
      &exact,
      cancel.clone(),
    )
    .await?;
    self.fence(&cancel)?;
    ensure!(
      operation.host(self).await? == selected_host,
      "browser runtime binding changed"
    );
    self.fence(&cancel)?;
    let result = tokio::select! {
      result = operation.dispatch(self, &selected_host, &cancel) => result,
      _ = cancel.cancelled() => return Err(RuntimeError::Uncertain(
        "browser dispatch cancelled; inspect the recorded outcome before acting again".into()).into()),
    };
    self.daemon.events.notify_waiters();
    result
  }
  async fn observe(&self, args: Value, cancel: CancellationToken) -> anyhow::Result<Value> {
    use agent_protocol::v1::{ObserveRequest, browser_service_server::BrowserService};
    input::keys(&args, &["tab_id", "screenshot"])?;
    let tab_id = input::optional_text(&args, "tab_id")?.unwrap_or_default();
    let screenshot = input::boolean(&args, "screenshot", false)?;
    let observation = if tab_id.is_empty() {
      ensure!(!screenshot, "select a tab for a screenshot");
      Value::Null
    } else {
      self.tab(&tab_id, false).await?;
      self.fence(&cancel)?;
      let response = tokio::select! {
        result = self.daemon.observe(tonic::Request::new(ObserveRequest { tab_id: tab_id.clone(), screenshot })) => result?,
        _ = cancel.cancelled() => anyhow::bail!("browser observation cancelled"),
      }.into_inner();
      self.fence(&cancel)?;
      self.tab(&tab_id, false).await?;
      ensure!(
        response.snapshot_id.len() <= 1024,
        "browser snapshot identifier exceeds limit"
      );
      json!({"tab_id":response.tab_id,"snapshot_id":response.snapshot_id,
        "revision":response.revision,"accessibility_json":response.accessibility_json,
        "screenshot_artifact":response.screenshot_artifact})
    };
    let routes = self.daemon.browser.routes.lock().await;
    let tabs: Vec<_> = routes
      .tabs
      .values()
      .filter(|tab| {
        routes
          .spaces
          .get(&tab.space_id)
          .is_some_and(|binding| binding.space.session_id == self.session_id)
      })
      .map(|tab| {
        let url = agent_browser::validate_url(&tab.url)
          .ok()
          .map(|mut url| {
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
          })
          .unwrap_or_default();
        json!({"tab_id":tab.id,"space_id":tab.space_id,
          "url":url.chars().take(2048).collect::<String>(),
          "title":tab.title.chars().take(512).collect::<String>()})
      })
      .collect();
    Ok(json!({"untrusted_page_content":true,"tabs":tabs,"observation":observation}))
  }
}
