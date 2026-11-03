use crate::Daemon;
use agent_core::events::{AGENT_EVENT_KIND, AgentEvent, ExecutionPhase};
use agent_core::{Run, RunStatus};
use agent_runtime::{RunOutcome, Runtime, RuntimeError};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
impl Daemon {
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
    let interaction = self.store.run_interaction_policy(&run.id).await?;
    plan
      .allowed_tools
      .retain(|tool| interaction.mode.permits(tool.name()));
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
    let (tools, skill_prompt) = self
      .execution_tools(crate::execution_tools::ToolScope {
        run,
        session,
        plan: &plan,
        permitted_tools,
        browser,
        computer: computer
          .as_ref()
          .map_or_else(Vec::new, |computer| computer.definitions()),
        delegation_targets,
        mode: interaction.mode,
      })
      .await?;
    let runtime_config = self
      .execution_config(session, skill_prompt, memory_context)
      .await?;
    let runtime = if let Some(backend) = backend {
      Runtime::open_with_backend(
        runtime_config,
        Arc::new(crate::run_input::RunPersistence {
          store: self.store.clone(),
          run_id: run.id.clone(),
        }),
        tools,
        listener,
        backend,
      )
      .await
    } else {
      Runtime::open(
        runtime_config,
        Arc::new(crate::run_input::RunPersistence {
          store: self.store.clone(),
          run_id: run.id.clone(),
        }),
        tools,
        listener,
      )
      .await
    };
    let runtime = match runtime {
      Ok(runtime) => Arc::new(runtime),
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
    let images = self.media_input(run).await?;
    let execute = runtime.execute_input(&run.prompt, images);
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
