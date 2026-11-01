use crate::Daemon;
use agent_model_hub::{HubClient, Runtime};
use agent_protocol::v1;
use tonic::Status;

impl Daemon {
  pub(crate) async fn execute_hub(&self, input: v1::HubRequest) -> Result<String, Status> {
    let root = self
      .config
      .data_path()
      .map_err(crate::service::db_error)?
      .join("models");
    let client = HubClient::new(
      &self.config.hub_endpoint,
      self.config.hub_token.as_deref(),
      root,
    )
    .map_err(hub_error)?;
    let cancel = self.stopping.child_token();
    let value = match input.action.as_str() {
      "search" => serde_json::to_value(
        client
          .search(&input.repository, 30, &cancel)
          .await
          .map_err(hub_error)?,
      ),
      "inspect" => serde_json::to_value(
        client
          .inspect(&input.repository, revision(&input), &cancel)
          .await
          .map_err(hub_error)?,
      ),
      "plan" | "install" => {
        let model = client
          .inspect(&input.repository, revision(&input), &cancel)
          .await
          .map_err(hub_error)?;
        let runtime = match input.runtime.as_str() {
          "llama_cpp" => Runtime::LlamaCpp,
          "mistral_rs" => Runtime::MistralRs,
          _ => {
            return Err(Status::invalid_argument(
              "runtime must be llama_cpp or mistral_rs",
            ));
          }
        };
        let plan = agent_model_hub::plan(&model, &input.file, runtime).map_err(hub_error)?;
        if input.action == "plan" {
          serde_json::to_value(plan)
        } else {
          if input.id != plan.id {
            return Err(Status::failed_precondition(
              "inspect installation plan and pass its id before downloading",
            ));
          }
          Ok(
            self
              .start_model_install(client, plan)
              .await
              .map_err(hub_error)?,
          )
        }
      }
      "installed" => serde_json::to_value(client.installed().map_err(hub_error)?),
      "status" => Ok(
        self
          .store
          .model_job(&input.id)
          .map_err(crate::service::db_error)?
          .unwrap_or_else(|| serde_json::json!({"id":input.id,"state":"unknown"})),
      ),
      "cancel" => {
        if let Some(token) = self.model_jobs.lock().await.get(&input.id) {
          token.cancel();
        }
        Ok(serde_json::json!({"id":input.id,"state":"cancel_requested"}))
      }
      "load" => {
        let installed = client
          .installed()
          .map_err(hub_error)?
          .into_iter()
          .find(|model| model.plan.id == input.id)
          .ok_or_else(|| Status::not_found("installed model not found"))?;
        match installed.plan.runtime {
          Runtime::LlamaCpp if !cfg!(feature = "local-llama") => {
            return Err(Status::failed_precondition(
              "llama backend unavailable; build agentd with local-llama",
            ));
          }
          Runtime::MistralRs if !cfg!(feature = "local-mistral") => {
            return Err(Status::failed_precondition(
              "mistral backend unavailable; build agentd with local-mistral",
            ));
          }
          _ => {}
        }
        let profile = agent_config::LocalModelSettings {
          id: installed.plan.id.clone(),
          path: installed.entrypoint,
          backend: match installed.plan.runtime {
            Runtime::LlamaCpp => agent_config::LocalModelBackend::Llama,
            Runtime::MistralRs => agent_config::LocalModelBackend::Mistral,
          },
          format: if installed.plan.format == "gguf" {
            agent_config::LocalModelFormat::Gguf
          } else {
            agent_config::LocalModelFormat::Directory
          },
          max_model_bytes: installed.plan.total_bytes,
          supports_tools: self.config.local_model_tools,
          chat_template: self.config.local_model_chat_template.clone(),
          ..Default::default()
        };
        serde_json::to_value(self.models.install(&profile).await.map_err(hub_error)?)
      }
      "remove" => {
        if self.models.backend(&input.id).await.is_ok() {
          return Err(Status::failed_precondition(
            "loaded model cannot be removed",
          ));
        }
        client.remove(&input.id).map_err(hub_error)?;
        Ok(serde_json::json!({"removed":input.id}))
      }
      "unload" => {
        let _guard = self.submission.lock().await;
        if self.active_count().await > 0 {
          return Err(Status::failed_precondition(
            "cannot unload models while runs or tasks are active",
          ));
        }
        if self
          .config
          .local_models
          .iter()
          .any(|profile| profile.id == input.id)
        {
          return Err(Status::failed_precondition(
            "configured models require configuration change and restart",
          ));
        }
        let removed = self.models.unload(&input.id).await.map_err(hub_error)?;
        Ok(serde_json::json!({"id":input.id,"unloaded":removed}))
      }
      _ => return Err(Status::invalid_argument("unknown Hub operation")),
    }
    .map_err(|_| Status::internal("Hub serialization failed"))?;
    Ok(value.to_string())
  }
}
fn revision(input: &v1::HubRequest) -> &str {
  if input.revision.is_empty() {
    "main"
  } else {
    &input.revision
  }
}
fn hub_error(_: impl std::fmt::Display) -> Status {
  // SDK transport errors may include presigned CDN URLs. Keep them out of UI replies.
  Status::failed_precondition(
    "Model Hub operation failed; check source, credentials, artifact compatibility and available disk space",
  )
}
