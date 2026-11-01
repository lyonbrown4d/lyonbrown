use crate::Daemon;
use agent_protocol::v1::{
  self,
  model_service_server::{ModelService, ModelServiceServer},
};
use agent_security::Authenticate;
use agent_storage::ModelBinding;
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};

pub fn model_service(
  daemon: Daemon,
) -> InterceptedService<ModelServiceServer<Daemon>, Authenticate> {
  let auth = Authenticate::new(daemon.auth_token());
  ModelServiceServer::with_interceptor(daemon, auth)
}

impl Daemon {
  pub(crate) fn model_bindings(&self) -> Vec<(String, String, String, ModelBinding)> {
    let mut entries = vec![(
      self.config.model.clone(),
      self.config.model.clone(),
      self.config.provider.clone(),
      ModelBinding {
        model_id: self.config.model.clone(),
        execution_id: "configured".into(),
        provider: self.config.provider.clone(),
        model: self.config.model.clone(),
        endpoint: self.config.api_base.clone(),
        credential_id: agent_config::provider_definition(&self.config.provider)
          .map_or(self.config.provider.as_str(), |entry| entry.id)
          .into(),
      },
    )];
    for (id, name, vendor, protocol, endpoint) in [
      (
        "deepseek-chat",
        "DeepSeek Chat",
        "deepseek",
        "openai-completions",
        "https://api.deepseek.com",
      ),
      (
        "deepseek-reasoner",
        "DeepSeek Reasoner",
        "deepseek",
        "openai-completions",
        "https://api.deepseek.com",
      ),
      (
        "gpt-4.1",
        "GPT-4.1",
        "openai",
        "openai-responses",
        "https://api.openai.com/v1",
      ),
      (
        "claude-sonnet-4-5",
        "Claude Sonnet 4.5",
        "anthropic",
        "anthropic",
        "https://api.anthropic.com",
      ),
    ] {
      if entries.iter().any(|entry| entry.0 == id) {
        continue;
      }
      entries.push((
        id.into(),
        name.into(),
        vendor.into(),
        ModelBinding {
          model_id: id.into(),
          execution_id: format!("builtin:{id}"),
          provider: protocol.into(),
          model: id.into(),
          endpoint: Some(endpoint.into()),
          credential_id: vendor.into(),
        },
      ));
    }
    for profile in &self.config.local_models {
      if entries.iter().any(|entry| entry.0 == profile.id) {
        continue;
      }
      entries.push((
        profile.id.clone(),
        profile.id.clone(),
        "Imported".into(),
        ModelBinding {
          model_id: profile.id.clone(),
          execution_id: format!("local:{}", profile.id),
          provider: "local".into(),
          model: profile.id.clone(),
          endpoint: None,
          credential_id: String::new(),
        },
      ));
    }
    for info in self.models.all() {
      if entries.iter().any(|entry| entry.0 == info.id) {
        continue;
      }
      entries.push((
        info.id.clone(),
        info.id.clone(),
        "Hugging Face".into(),
        ModelBinding {
          model_id: info.id.clone(),
          execution_id: format!("local:{}", info.id),
          provider: "local".into(),
          model: info.id,
          endpoint: None,
          credential_id: String::new(),
        },
      ));
    }
    entries
  }
  pub(crate) fn binding_credential(
    &self,
    binding: &ModelBinding,
  ) -> anyhow::Result<Option<String>> {
    use agent_secrets::ExposeSecret;
    if let Some(value) = self.config.catalog_credentials.get(&binding.credential_id) {
      return Ok(Some(value.clone()));
    }
    if binding.execution_id == "configured"
      && let Some(key) = &self.config.provider_api_key
      && !key.is_empty()
    {
      return Ok(Some(key.clone()));
    }
    Ok(
      self
        .provider_secrets
        .get(&format!("provider-{}", binding.credential_id))?
        .map(|secret| secret.expose_secret().to_owned()),
    )
  }
  pub(crate) fn configured_for_run(&self, run: &str) -> anyhow::Result<Self> {
    let Some(binding) = self.store.model_binding(run)? else {
      return Ok(self.clone());
    };
    let mut daemon = self.clone();
    let mut config = self.config.clone();
    config.provider = binding.provider.clone();
    config.model = binding.model.clone();
    config.api_base = binding.endpoint.clone();
    config.provider_api_key = self.binding_credential(&binding)?;
    daemon.config = config;
    Ok(daemon)
  }
}

#[tonic::async_trait]
impl ModelService for Daemon {
  async fn hub_operation(
    &self,
    request: Request<v1::HubRequest>,
  ) -> Result<Response<v1::HubResponse>, Status> {
    self
      .execute_hub(request.into_inner())
      .await
      .map(|json| Response::new(v1::HubResponse { json }))
  }
  async fn list_models(
    &self,
    request: Request<v1::SessionRequest>,
  ) -> Result<Response<v1::Models>, Status> {
    let id = request.into_inner().session_id;
    self
      .store
      .session(&id)
      .map_err(crate::service::db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    let selected = self
      .store
      .model_binding(&id)
      .map_err(crate::service::db_error)?;
    let mut models = Vec::new();
    for (id, name, vendor, binding) in self.model_bindings() {
      let ready = if binding.provider == "local" {
        self.models.backend(&binding.model).await.is_ok()
      } else {
        self
          .binding_credential(&binding)
          .map_err(crate::service::db_error)?
          .is_some()
      };
      let state = if ready {
        "ready"
      } else if binding.provider == "local" {
        "unavailable"
      } else {
        "configurable"
      };
      models.push(v1::ModelCandidate {
        id,
        display_name: name,
        vendor,
        source: if binding.execution_id == "configured" {
          "configured"
        } else {
          "builtin"
        }
        .into(),
        state: state.into(),
        selected: selected
          .as_ref()
          .map_or(binding.execution_id == "configured", |value| {
            value.execution_id == binding.execution_id
          }),
        executions: vec![v1::ModelExecution {
          id: binding.execution_id,
          label: if binding.provider == "local" {
            "Run on this machine"
          } else {
            "API"
          }
          .into(),
          transport: if binding.provider == "local" {
            "local_inference"
          } else {
            "api"
          }
          .into(),
          state: state.into(),
          reason: if ready {
            ""
          } else {
            "Configure credential or load model"
          }
          .into(),
        }],
      });
    }
    let hub = agent_model_hub::HubClient::new(
      &self.config.hub_endpoint,
      self.config.hub_token.as_deref(),
      self
        .config
        .data_path()
        .map_err(crate::service::db_error)?
        .join("models"),
    )
    .map_err(crate::service::db_error)?;
    for installed in hub.installed().map_err(crate::service::db_error)? {
      if let Some(entry) = models
        .iter_mut()
        .find(|entry| entry.id == installed.plan.id)
      {
        entry.display_name = installed.plan.repository.clone();
        entry.vendor = installed
          .plan
          .repository
          .split('/')
          .next()
          .unwrap_or("Hugging Face")
          .into();
        entry.source = "installed".into();
        continue;
      }
      models.push(v1::ModelCandidate {
        id: installed.plan.id.clone(),
        display_name: installed.plan.repository,
        vendor: "Hugging Face".into(),
        source: "installed".into(),
        state: "installed_unverified".into(),
        selected: false,
        executions: vec![v1::ModelExecution {
          id: format!("local:{}", installed.plan.id),
          label: "Run on this machine".into(),
          transport: "local_inference".into(),
          state: "installed_unverified".into(),
          reason: "Load and verify the inference backend before selecting".into(),
        }],
      });
    }
    Ok(Response::new(v1::Models { models }))
  }
  async fn select_model(
    &self,
    request: Request<v1::SelectModelRequest>,
  ) -> Result<Response<v1::ModelSelection>, Status> {
    let input = request.into_inner();
    let _guard = self.submission.lock().await;
    let session = self
      .store
      .session(&input.session_id)
      .map_err(crate::service::db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::failed_precondition(
        "session is owned by another node",
      ));
    }
    let binding = self
      .model_bindings()
      .into_iter()
      .map(|entry| entry.3)
      .find(|binding| {
        binding.model_id == input.model_id && binding.execution_id == input.execution_id
      })
      .ok_or_else(|| Status::not_found("model execution not found"))?;
    if binding.provider == "local" {
      self
        .models
        .backend(&binding.model)
        .await
        .map_err(|_| Status::failed_precondition("model is not loaded"))?;
    } else if self
      .binding_credential(&binding)
      .map_err(crate::service::db_error)?
      .is_none()
    {
      return Err(Status::failed_precondition("model credential required"));
    }
    self
      .store
      .select_model(&input.session_id, input.owner_epoch, &binding)
      .map_err(crate::service::db_error)?;
    Ok(Response::new(v1::ModelSelection {
      model_id: binding.model_id,
      execution_id: binding.execution_id,
    }))
  }
}
