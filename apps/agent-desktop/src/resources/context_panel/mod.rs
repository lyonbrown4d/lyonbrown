mod view;
use agent_client::AgentClient;
use agent_protocol::v1;
use gpui_kit::*;
use std::sync::Arc;
#[derive(Clone, Copy)]
enum Command {
  Refresh,
  Summary,
  Compact,
  Undo,
  Cancel,
  Abandon,
  Mode(v1::InteractionMode),
}
pub(super) struct ContextPanel {
  runtime: Arc<tokio::runtime::Runtime>,
  client: Option<AgentClient>,
  session: Option<String>,
  generation: u64,
  pub(super) pending: bool,
  context: Option<v1::ContextStatus>,
  maintenance: Option<v1::ContextMaintenanceJob>,
  policy: Option<v1::InteractionPolicy>,
  summary: String,
  message: String,
  abandon_request: Option<(String, String)>,
}
impl ContextPanel {
  pub(super) fn new(runtime: Arc<tokio::runtime::Runtime>) -> Self {
    Self {
      runtime,
      client: None,
      session: None,
      generation: 0,
      pending: false,
      context: None,
      maintenance: None,
      policy: None,
      summary: String::new(),
      message: String::new(),
      abandon_request: None,
    }
  }
  pub(super) fn bind(
    &mut self,
    client: Option<AgentClient>,
    session: Option<String>,
    cx: &mut Context<Self>,
  ) {
    self.generation = self.generation.saturating_add(1);
    self.client = client;
    self.session = session;
    self.context = None;
    self.maintenance = None;
    self.policy = None;
    self.summary.clear();
    self.message.clear();
    if !self.pending {
      self.dispatch(Command::Refresh, cx);
    }
    cx.notify();
  }
  fn dispatch(&mut self, command: Command, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    if self.pending {
      return;
    }
    let revision = self.context.as_ref().map(|c| c.revision);
    let maintenance = self.maintenance.clone();
    let abandon_request = if matches!(command, Command::Abandon) {
      if let Some(job) = &maintenance
        && self
          .abandon_request
          .as_ref()
          .is_none_or(|(id, _)| id != &job.id)
      {
        self.abandon_request = Some((job.id.clone(), uuid::Uuid::now_v7().to_string()));
      }
      self.abandon_request.clone()
    } else {
      None
    };
    let mode_revision = self.policy.as_ref().map(|p| p.revision);
    if !matches!(
      command,
      Command::Refresh | Command::Summary | Command::Cancel | Command::Abandon
    ) && (revision.is_none() || mode_revision.is_none())
    {
      self.message = "Refresh state before changing this session.".into();
      cx.notify();
      return;
    }
    self.pending = true;
    self.message = "Waiting for agentd…".into();
    let generation = self.generation;
    let request = self.runtime.spawn(async move {
      let mut summary = None;
      match command {
        Command::Refresh => {}
        Command::Summary => summary = Some(client.context_summary(&session).await?.text),
        Command::Abandon => {
          let job = maintenance
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Context job unavailable"))?;
          let (_, request) =
            abandon_request.ok_or_else(|| anyhow::anyhow!("Resolution identity unavailable"))?;
          client
            .abandon_context(
              &job.id,
              job.owner_epoch,
              &request,
              "User closed the uncertain context session without replay.",
            )
            .await?;
        }
        Command::Cancel => {
          let job = maintenance.ok_or_else(|| anyhow::anyhow!("Context job unavailable"))?;
          client.cancel_context(&job.id, job.owner_epoch).await?;
        }
        _ => {
          let owner = client
            .runtime_status(&session)
            .await?
            .session
            .ok_or_else(|| anyhow::anyhow!("Session unavailable"))?;
          match command {
            Command::Compact => {
              client
                .compact_context(
                  &session,
                  owner.owner_epoch,
                  revision.unwrap_or_default(),
                  "Preserve the task, constraints, decisions and remaining work.",
                )
                .await?;
            }
            Command::Undo => {
              client
                .undo_context_compaction(&session, owner.owner_epoch, revision.unwrap_or_default())
                .await?;
            }
            Command::Mode(mode) => {
              client
                .set_interaction_mode(
                  &session,
                  owner.owner_epoch,
                  mode_revision.unwrap_or_default(),
                  mode,
                )
                .await?;
            }
            _ => {}
          }
        }
      }
      Ok::<_, anyhow::Error>((
        client.context_status(&session).await.ok(),
        client.interaction_policy(&session).await?,
        summary,
        client.context_maintenance(&session).await.ok(),
      ))
    });
    cx.spawn(async move |view, cx| {
      let result = request.await;
      let _ = view.update(cx, |view, cx| {
        view.pending = false;
        if view.generation == generation {
          match result {
            Ok(Ok((context, policy, summary, maintenance))) => {
              view.context = context;
              view.maintenance = maintenance;
              view.policy = Some(policy);
              view.summary = summary.unwrap_or_default();
              view.message = if view.context.is_none() {
                "Context statistics unavailable; saved maintenance can still be inspected.".into()
              } else {
                String::new()
              };
            }
            _ => {
              view.context = None;
              view.policy = None;
              view.summary.clear();
              view.message =
                "Command could not be confirmed. Refresh and inspect recovery before retrying."
                  .into();
            }
          }
        }
        if view.generation != generation {
          view.dispatch(Command::Refresh, cx);
        }
        cx.notify();
      });
    })
    .detach();
    cx.notify();
  }
}
