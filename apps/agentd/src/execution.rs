use crate::Daemon;
use agent_core::events::{AGENT_EVENT_KIND, AgentEvent, ExecutionPhase};
use agent_core::{Run, RunStatus};
use agent_runtime::{RunOutcome, Runtime, RuntimeError};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
impl Daemon {
  pub(crate) async fn launch(&self, run: Run) -> Result<()> {
    let session = self
      .store
      .session(&run.session_id)
      .await?
      .ok_or_else(|| anyhow::anyhow!("session missing"))?;
    let cancel = CancellationToken::new();
    self
      .active
      .lock()
      .await
      .insert(run.id.clone(), cancel.clone());
    let daemon = self.clone();
    tokio::spawn(async move {
      let permit = tokio::select! {value=daemon.permits.clone().acquire_owned()=>value.ok(),_=cancel.cancelled()=>None};
      if permit.is_none() {
        let _ = daemon
          .store
          .update_run_owned(&run.id, session.owner_epoch, RunStatus::Cancelled, "", "")
          .await;
      } else if let Err(error) = daemon
        .execute_run_scoped(&run, &session, cancel, None)
        .await
      {
        let status = if matches!(
          error.downcast_ref::<RuntimeError>(),
          Some(RuntimeError::Uncertain(_) | RuntimeError::Persistence(_))
        ) {
          RunStatus::Uncertain
        } else {
          RunStatus::Failed
        };
        if let Err(storage) = daemon
          .store
          .update_run_owned(
            &run.id,
            session.owner_epoch,
            status,
            "",
            &agent_storage::public_error_message(&error),
          )
          .await
        {
          tracing::error!(run_id=%run.id,error=%agent_storage::public_error_message(&storage),"could not persist failed run");
        }
      }
      drop(permit);
      daemon.active.lock().await.remove(&run.id);
      daemon.events.notify_waiters();
    });
    Ok(())
  }
  pub(crate) async fn execute_run_scoped(
    &self,
    run: &Run,
    session: &agent_core::Session,
    cancel: CancellationToken,
    permitted_tools: Option<&std::collections::BTreeSet<String>>,
  ) -> Result<()> {
    let configured = self.configured_for_run(&run.id).await?;
    configured
      .execute_selected_run(run, session, cancel, permitted_tools)
      .await
  }
  async fn execute_selected_run(
    &self,
    run: &Run,
    session: &agent_core::Session,
    cancel: CancellationToken,
    permitted_tools: Option<&std::collections::BTreeSet<String>>,
  ) -> Result<()> {
    let model = self.execution_model_identity(session)?;
    self
      .store
      .append_event_owned(
        &run.id,
        session.owner_epoch,
        AGENT_EVENT_KIND,
        serde_json::to_value(AgentEvent::PhaseChanged {
          phase: ExecutionPhase::Preparing,
          model: Some(model.clone()),
        })?,
      )
      .await?;
    self
      .store
      .update_run_owned(&run.id, session.owner_epoch, RunStatus::Running, "", "")
      .await?;
    self.events.notify_waiters();
    self
      .record_model_route(run, session.owner_epoch, &cancel)
      .await?;
    let browser = self.config.browser_tools && self.browser.has_runtime().await;
    let computer = crate::computer::tools(self.clone(), run, session, cancel.clone()).await;
    let commands = crate::commands::enabled(&self.config);
    let delegation_targets = crate::delegation::available_targets(self).await?;
    let mut capabilities = [
      agent_decision::Capability::FileRead,
      agent_decision::Capability::FileWrite,
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    if browser {
      capabilities.insert(agent_decision::Capability::Browser);
    }
    if computer.is_some() {
      capabilities.insert(agent_decision::Capability::Computer);
    }
    if commands {
      capabilities.insert(agent_decision::Capability::ProcessExecute);
    }
    if !delegation_targets.is_empty() {
      capabilities.insert(agent_decision::Capability::Delegation);
    }
    if !self.config.mcp.is_empty() {
      capabilities.insert(agent_decision::Capability::Mcp);
    }
    let decision = self
      .decision
      .with_available_capabilities(&capabilities)
      .with_available_targets(&delegation_targets);
    let explicit = self.config.decision.model.is_none().then(|| {
      crate::decision::workspace_rule(
        !self.config.mcp.is_empty(),
        browser,
        commands,
        !delegation_targets.is_empty(),
        computer.is_some(),
      )
    });
    let mut plan = match decision.decide(&run.prompt, explicit, cancel.clone()).await {
      Ok(plan) => plan,
      Err(agent_decision::DecisionError::Cancelled) => {
        self
          .store
          .update_run_owned(&run.id, session.owner_epoch, RunStatus::Cancelled, "", "")
          .await?;
        return Ok(());
      }
      Err(error) => return Err(error.into()),
    };
    let proposed_tools = plan.allowed_tools.clone();
    if let Some(permitted) = permitted_tools {
      plan
        .allowed_tools
        .retain(|tool| permitted.contains(tool.name()));
    }
    let backend = if self.config.provider == "local" {
      let backend = self.models.backend(&self.config.model).await?;
      let info = backend.info();
      if !info.supports_tools {
        plan.allowed_tools.clear();
        plan.required_capabilities.clear();
      }
      self
        .store
        .append_event_owned(
          &run.id,
          session.owner_epoch,
          "model_selected",
          serde_json::to_value(&info)?,
        )
        .await?;
      Some(backend)
    } else {
      None
    };
    if plan.allowed_tools != proposed_tools
      && let agent_decision::DecisionSource::Classifier { restricted } = &mut plan.source
    {
      *restricted = true;
    }
    // Persist the effective, authority-clamped plan before opening the Rig runtime.
    self
      .store
      .append_event_owned(
        &run.id,
        session.owner_epoch,
        "execution_plan",
        serde_json::to_value(&plan)?,
      )
      .await?;
    if cancel.is_cancelled() {
      self
        .store
        .update_run_owned(&run.id, session.owner_epoch, RunStatus::Cancelled, "", "")
        .await?;
      return Ok(());
    }
    let memory_context = self.memory_context(run, session, &cancel).await?;
    if cancel.is_cancelled() {
      self
        .store
        .update_run_owned(&run.id, session.owner_epoch, RunStatus::Cancelled, "", "")
        .await?;
      return Ok(());
    }
    let (event_writer, listener) = crate::execution_event_writer::EventWriter::start(
      self.store.clone(),
      run.id.clone(),
      session.owner_epoch,
      model,
      self.events.clone(),
      cancel.clone(),
    );
    let mut tools =
      crate::tools::workspace_tools(self.clone(), run, &session.workspace, session.owner_epoch)?;
    let (skill_prompt, skill_tool) = crate::skill_runtime::prepare(self, run, session).await?;
    if let Some(tool) = skill_tool {
      tools.push(tool);
    }
    if let Some(tool) = self.mcp.tool(self.clone(), run, session.owner_epoch) {
      tools.push(tool);
    }
    if browser {
      tools.extend(crate::browser::tools(self.clone(), run, session));
    }
    tools.extend(crate::commands::tools(self.clone(), run, session)?);
    if let Some(computer) = &computer {
      tools.extend(computer.definitions());
    }
    if plan.execution_mode == agent_decision::ExecutionMode::DistributedAllowed {
      let targets = delegation_targets
        .into_iter()
        .filter(|target| plan.target_hint.as_ref().is_none_or(|hint| hint == target))
        .collect();
      let scoped_capabilities = [
        (agent_decision::Tool::ReadFile, "workspace.read"),
        (agent_decision::Tool::ListFiles, "workspace.read"),
        (agent_decision::Tool::SearchFiles, "workspace.read"),
        (agent_decision::Tool::WriteFile, "workspace.write.approval"),
      ]
      .into_iter()
      .filter(|(tool, _)| plan.allowed_tools.contains(tool))
      .map(|(_, capability)| capability.to_owned())
      .collect::<std::collections::BTreeSet<_>>()
      .into_iter()
      .collect();
      tools.extend(crate::delegation::tools(
        self.clone(),
        run,
        session,
        targets,
        scoped_capabilities,
      ));
    }
    tools.retain(|tool| {
      tool.definition().name == "load_skill"
        || permitted_tools.is_none_or(|permitted| permitted.contains(&tool.definition().name))
          && plan
            .allowed_tools
            .iter()
            .any(|allowed| allowed.name() == tool.definition().name)
    });
    let mut runtime_config = self.config.runtime(session);
    if runtime_config.api_key.is_empty()
      && let Some(secret) = self
        .provider_secrets
        .get(&format!(
          "provider-{}",
          agent_config::provider_definition(&self.config.provider)
            .map_or(self.config.provider.as_str(), |entry| entry.id)
        ))
        .map_err(|_| anyhow::anyhow!("provider credential store unavailable"))?
    {
      use agent_secrets::ExposeSecret;
      runtime_config.api_key = secret.expose_secret().to_owned();
    }
    crate::repository_instructions::apply(&mut runtime_config, &session.workspace).await?;
    if !skill_prompt.is_empty() {
      runtime_config
        .system_prompt
        .get_or_insert_default()
        .push_str(&skill_prompt);
    }
    if let Some(memory_context) = memory_context {
      runtime_config
        .system_prompt
        .get_or_insert_default()
        .push_str(&memory_context);
    }
    let runtime = if let Some(backend) = backend {
      let info = self.models.info(&self.config.model)?;
      runtime_config.context_window = info.context_window;
      runtime_config.max_output_tokens = self
        .config
        .local_models
        .iter()
        .find(|profile| profile.id == self.config.model)
        .map_or(512.min(info.context_window.saturating_sub(1)), |profile| {
          profile.max_output_tokens
        });
      runtime_config.supports_images = info.supports_images;
      Runtime::open_with_backend(
        runtime_config,
        Arc::new(crate::run_input::RunPersistence {
          store: self.store.clone(),
          private_store: self.provider_secrets.clone(),
          run_id: run.id.clone(),
        }),
        tools,
        listener,
        backend,
      )
      .await
    } else {
      self
        .open_remote_runtime(
          runtime_config,
          Arc::new(crate::run_input::RunPersistence {
            store: self.store.clone(),
            private_store: self.provider_secrets.clone(),
            run_id: run.id.clone(),
          }),
          tools,
          listener,
        )
        .await
    };
    let runtime = match runtime {
      Ok(runtime) => Arc::new(runtime.with_privacy(self.privacy.clone()).await?),
      Err(error) => {
        event_writer.drain().await?;
        return Err(error.into());
      }
    };
    {
      let mut registered = self.input_runtimes.lock().await;
      if registered.len() >= 128 && !registered.contains_key(&session.id) {
        event_writer.drain().await?;
        anyhow::bail!(
          "input runtime capacity reached; reconcile pending queues before starting another session"
        );
      }
      registered.insert(
        session.id.clone(),
        crate::run_input::RunRuntime {
          run_id: run.id.clone(),
          epoch: session.owner_epoch,
          runtime: runtime.clone(),
        },
      );
    }
    let execute = runtime.execute(&run.prompt);
    tokio::pin!(execute);
    let result = async { tokio::select! {
      biased;
      _=cancel.cancelled()=>{runtime.cancel().await?;tokio::time::timeout(Duration::from_secs(10),&mut execute).await.map_err(|_|RuntimeError::Uncertain("cancellation did not reach a durable terminal result".into()))?},
      result=&mut execute=>result,
      _=tokio::time::sleep(Duration::from_secs(self.config.run_timeout_secs))=>{runtime.cancel().await?;tokio::time::timeout(Duration::from_secs(10),&mut execute).await.map_err(|_|RuntimeError::Uncertain("execution deadline expired with unknown outcome".into()))?},
    }}.await;
    runtime.close_input_admission().await;
    if !runtime.has_pending_inputs().await.unwrap_or(true) {
      self.input_runtimes.lock().await.remove(&session.id);
    }
    if let Some(computer) = &computer {
      computer.close().await;
    }
    event_writer.drain().await?;
    let result = result?;
    let (status, error) = match result.outcome {
      RunOutcome::Completed => (RunStatus::Completed, String::new()),
      RunOutcome::Cancelled => (RunStatus::Cancelled, String::new()),
      RunOutcome::Failed { message } => (RunStatus::Failed, message),
      RunOutcome::Uncertain { message } => (RunStatus::Uncertain, message),
    };
    self
      .store
      .update_run_owned(&run.id, session.owner_epoch, status, &result.text, &error)
      .await?;
    self.events.notify_waiters();
    Ok(())
  }
}
