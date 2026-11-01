use super::ResourceSection;
use agent_client::AgentClient;
use agent_protocol::v1;
use gpui_kit::component::{
  input::{InputEvent, InputState},
  select::{SelectEvent, SelectState},
};
use gpui_kit::*;
use std::{collections::BTreeMap, sync::Arc};

pub(super) enum Reply {
  Models(Vec<v1::ModelCandidate>),
  Providers(Vec<v1::Provider>),
  Skills(Vec<v1::Skill>),
  Status(Box<v1::RuntimeStatus>),
  InspectedSkill(String, String, v1::SkillInspection),
  Hub(v1::HubRequest, serde_json::Value),
  Changed(String),
}
pub(super) struct Completion {
  pub generation: u64,
  pub input_revision: u64,
  pub mutation: bool,
  pub result: anyhow::Result<Reply>,
}
pub(crate) struct ResourcePane {
  pub(super) section: ResourceSection,
  pub(super) runtime: Arc<tokio::runtime::Runtime>,
  pub(super) client: Option<AgentClient>,
  pub(super) session: Option<String>,
  pub(super) generation: u64,
  pub(super) input_revision: u64,
  pub(super) sender: async_channel::Sender<Completion>,
  pub(super) busy: bool,
  pub(crate) pending_mutations: usize,
  pub(super) error: Option<String>,
  pub(super) notice: Option<String>,
  pub(super) models: Vec<v1::ModelCandidate>,
  pub(super) providers: Vec<v1::Provider>,
  pub(super) skills: Vec<v1::Skill>,
  pub(super) status: Option<v1::RuntimeStatus>,
  pub(super) environment: BTreeMap<String, agent_config::ProviderCredentialStatus>,
  pub(super) source: Entity<InputState>,
  pub(super) scope: Entity<SelectState<Vec<String>>>,
  pub(super) skill_scope: String,
  pub(super) skill_receipt: Option<(String, String, v1::SkillInspection)>,
  pub(super) query: Entity<InputState>,
  pub(super) job_id: Entity<InputState>,
  pub(super) repository: Entity<InputState>,
  pub(super) revision: Entity<InputState>,
  pub(super) file: Entity<InputState>,
  pub(super) backend: Entity<SelectState<Vec<String>>>,
  pub(super) hub_inspection: Option<(v1::HubRequest, serde_json::Value)>,
  pub(super) hub_plan: Option<(v1::HubRequest, serde_json::Value)>,
  pub(super) hub_results: Vec<serde_json::Value>,
  pub(super) installed: Vec<serde_json::Value>,
  pub(super) jobs: BTreeMap<String, serde_json::Value>,
  pub(super) advanced: bool,
  _subscriptions: Vec<Subscription>,
  _completion: Task<()>,
}
impl ResourcePane {
  pub(super) fn new(
    section: ResourceSection,
    runtime: Arc<tokio::runtime::Runtime>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let (sender, receiver) = crate::completion::channel::<Completion>(32);
    let completion = cx.spawn_in(window, async move |view, cx| {
      while let Ok(completion) = receiver.recv().await {
        if view
          .update_in(cx, |view, _, cx| {
            if completion.mutation {
              view.pending_mutations = view.pending_mutations.saturating_sub(1);
            }
            if completion.generation == view.generation {
              view.busy = false;
              let inspection = match &completion.result {
                Ok(Reply::InspectedSkill(..)) => true,
                Ok(Reply::Hub(request, _)) => {
                  matches!(request.action.as_str(), "search" | "inspect" | "plan")
                }
                _ => false,
              };
              if !inspection || completion.input_revision == view.input_revision {
                view.apply(completion.result, cx);
              }
            } else if completion.mutation && view.pending_mutations == 0 && !view.busy {
              view.refresh(cx);
            }
            cx.notify();
          })
          .is_err()
        {
          break;
        }
      }
    });
    let source = cx.new(|cx| {
      InputState::new(window, cx).placeholder("Skill directory, archive or URL on agentd")
    });
    let job_id =
      cx.new(|cx| InputState::new(window, cx).placeholder("Previously saved installation job ID"));
    let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search Hugging Face models"));
    let repository =
      cx.new(|cx| InputState::new(window, cx).placeholder("Repository: owner/model"));
    let revision = cx.new(|cx| {
      InputState::new(window, cx)
        .default_value("main")
        .placeholder("Revision")
    });
    let file =
      cx.new(|cx| InputState::new(window, cx).placeholder("Artifact path from inspected files"));
    let scope = cx.new(|cx| {
      SelectState::new(
        vec!["daemon_user".into(), "workspace".into()],
        Some(gpui_kit::base::IndexPath::new(0)),
        window,
        cx,
      )
    });
    let backend = cx.new(|cx| {
      SelectState::new(
        vec!["llama_cpp".into(), "mistral_rs".into()],
        Some(gpui_kit::base::IndexPath::new(0)),
        window,
        cx,
      )
    });
    let mut subscriptions = Vec::new();
    for input in [&source, &query, &repository, &revision, &file] {
      subscriptions.push(cx.observe(input, |_, _, cx| cx.notify()));
    }
    for input in [&source, &query, &repository, &revision] {
      subscriptions.push(cx.subscribe(input, |this, _, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
          this.input_revision = this.input_revision.saturating_add(1);
          this.skill_receipt = None;
          this.hub_plan = None;
          this.hub_inspection = None;
          cx.notify();
        }
      }));
    }
    subscriptions.push(
      cx.subscribe(&scope, |this, _, _: &SelectEvent<Vec<String>>, cx| {
        this.skill_scope = this.scope_name(cx);
        this.generation = this.generation.saturating_add(1);
        this.busy = false;
        this.skill_receipt = None;
        this.skills.clear();
        this.refresh(cx);
        cx.notify();
      }),
    );
    subscriptions.push(cx.subscribe(&file, |this, _, event: &InputEvent, cx| {
      if matches!(event, InputEvent::Change) {
        this.input_revision = this.input_revision.saturating_add(1);
        this.hub_plan = None;
        cx.notify();
      }
    }));
    subscriptions.push(
      cx.subscribe(&backend, |this, _, _: &SelectEvent<Vec<String>>, cx| {
        this.input_revision = this.input_revision.saturating_add(1);
        this.hub_plan = None;
        cx.notify();
      }),
    );
    Self {
      section,
      runtime,
      client: None,
      session: None,
      generation: 0,
      input_revision: 0,
      sender,
      busy: false,
      pending_mutations: 0,
      error: None,
      notice: None,
      models: Vec::new(),
      providers: Vec::new(),
      skills: Vec::new(),
      status: None,
      environment: agent_config::detect_provider_environment(),
      source,
      scope,
      skill_scope: "daemon_user".into(),
      skill_receipt: None,
      query,
      job_id,
      repository,
      revision,
      file,
      backend,
      hub_inspection: None,
      hub_plan: None,
      hub_results: Vec::new(),
      installed: Vec::new(),
      jobs: BTreeMap::new(),
      advanced: false,
      _subscriptions: subscriptions,
      _completion: completion,
    }
  }
  pub(super) fn bind(
    &mut self,
    client: Option<AgentClient>,
    session: Option<String>,
    cx: &mut Context<Self>,
  ) {
    let session = if self.section == ResourceSection::Providers {
      None
    } else {
      session
    };
    let connection_changed = self.client.is_some() != client.is_some();
    let preserve_hub =
      self.section == ResourceSection::Models && self.client.is_some() && !connection_changed;
    let changed = connection_changed || self.session != session;
    self.client = client;
    if !changed {
      return;
    }
    self.generation = self.generation.saturating_add(1);
    self.session = session;
    self.busy = false;
    self.error = None;
    self.notice = None;
    self.models.clear();
    self.providers.clear();
    self.skills.clear();
    self.status = None;
    self.skill_receipt = None;
    // Hub inventory and known jobs belong to this daemon, independently of the selected chat.
    // Connection replacement is routed through invalidate(None), which discards these snapshots.
    if !preserve_hub {
      self.hub_inspection = None;
      self.hub_plan = None;
      self.hub_results.clear();
      self.installed.clear();
      self.jobs.clear();
    }
    self.refresh(cx);
    cx.notify();
  }
  pub(super) fn ready(&self) -> bool {
    self.client.is_some()
      && ((self.section == ResourceSection::Providers || self.section == ResourceSection::Models)
        || (self.section == ResourceSection::Skills && self.skill_scope == "daemon_user")
        || self.session.is_some())
  }
  pub(super) fn scope_name(&self, cx: &App) -> String {
    self
      .scope
      .read(cx)
      .selected_value()
      .cloned()
      .unwrap_or_else(|| "daemon_user".into())
  }
}
