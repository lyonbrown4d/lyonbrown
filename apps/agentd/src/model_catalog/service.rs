use crate::Daemon;
use agent_protocol::v1::{
  self,
  model_service_server::{ModelService, ModelServiceServer},
};
use agent_security::Authenticate;
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};

pub fn model_service(
  daemon: Daemon,
) -> InterceptedService<ModelServiceServer<Daemon>, Authenticate> {
  let auth = Authenticate::new(daemon.auth_token());
  ModelServiceServer::with_interceptor(daemon, auth)
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
    let session_scoped = !id.is_empty();
    let selected = if session_scoped {
      self
        .store
        .session(&id)
        .await
        .map_err(crate::service::db_error)?
        .ok_or_else(|| Status::not_found("session not found"))?;
      self
        .store
        .model_binding(&id)
        .await
        .map_err(crate::service::db_error)?
    } else {
      None
    };
    let mut models = Vec::new();
    for (id, name, vendor, binding) in self
      .model_bindings()
      .await
      .map_err(crate::service::db_error)?
    {
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
        } else if binding.execution_id.starts_with("profile:") {
          "profile"
        } else {
          "builtin"
        }
        .into(),
        state: state.into(),
        selected: session_scoped
          && selected
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
      .await
      .map_err(crate::service::db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::failed_precondition(
        "session is owned by another node",
      ));
    }
    let binding = self
      .model_bindings()
      .await
      .map_err(crate::service::db_error)?
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
      .await
      .map_err(crate::service::db_error)?;
    Ok(Response::new(v1::ModelSelection {
      model_id: binding.model_id,
      execution_id: binding.execution_id,
    }))
  }
}
