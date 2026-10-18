use crate::Daemon;
use agent_core::{Run, RunStatus};
use agent_runtime::{RunOutcome, Runtime, RuntimeError, RuntimeEvent};
use anyhow::Result;
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
impl Daemon {
  pub(crate) async fn launch(&self, run: Run) -> Result<()> {
    let session = self
      .store
      .session(&run.session_id)?
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
        let _ =
          daemon
            .store
            .update_run_owned(&run.id, session.owner_epoch, RunStatus::Cancelled, "", "");
      } else if let Err(error) = daemon
        .execute_run_scoped(&run, &session, cancel, None)
        .await
      {
        let status = if matches!(
          error.downcast_ref::<RuntimeError>(),
          Some(RuntimeError::Uncertain(_))
        ) {
          RunStatus::Uncertain
        } else {
          RunStatus::Failed
        };
        if let Err(storage) = daemon.store.update_run_owned(
          &run.id,
          session.owner_epoch,
          status,
          "",
          &error.to_string(),
        ) {
          tracing::error!(run_id=%run.id,error=%storage,"could not persist failed run");
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
    self
      .store
      .update_run_owned(&run.id, session.owner_epoch, RunStatus::Running, "", "")?;
    self.events.notify_waiters();
    let browser = self.config.browser_tools && self.browser.has_runtime().await;
    let computer = crate::computer::tools(self.clone(), run, session, cancel.clone()).await;
    let commands = crate::commands::enabled(&self.config);
    let delegation_targets = crate::delegation::available_targets(self)?;
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
          .update_run_owned(&run.id, session.owner_epoch, RunStatus::Cancelled, "", "")?;
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
      self.store.append_event_owned(
        &run.id,
        session.owner_epoch,
        "model_selected",
        serde_json::to_value(&info)?,
      )?;
      Some(backend)
    } else {
      None
    };
    if plan.allowed_tools != proposed_tools
      && let agent_decision::DecisionSource::Classifier { restricted } = &mut plan.source
    {
      *restricted = true;
    }
    // Persist the effective, authority-clamped plan before opening the Harness.
    self.store.append_event_owned(
      &run.id,
      session.owner_epoch,
      "execution_plan",
      serde_json::to_value(&plan)?,
    )?;
    if cancel.is_cancelled() {
      self
        .store
        .update_run_owned(&run.id, session.owner_epoch, RunStatus::Cancelled, "", "")?;
      return Ok(());
    }
    let persist_failed = Arc::new(AtomicBool::new(false));
    let failed = persist_failed.clone();
    let epoch = session.owner_epoch;
    let store = self.store.clone();
    let run_id = run.id.clone();
    let notify = self.events.clone();
    let stop = cancel.clone();
    let listener = Arc::new(move |event: RuntimeEvent| {
      let result = (|| -> anyhow::Result<()> {
        let mut payload = serde_json::to_value(&event)?;
        let kind = payload
          .get("type")
          .and_then(|v| v.as_str())
          .unwrap_or("runtime_event")
          .to_owned();
        if let Some(map) = payload.as_object_mut() {
          map.remove("type");
        }
        store.append_event_owned(&run_id, epoch, &kind, payload)?;
        Ok(())
      })();
      if result.is_err() {
        failed.store(true, Ordering::Release);
        stop.cancel();
      }
      notify.notify_waiters();
    });
    let mut tools =
      crate::tools::workspace_tools(self.clone(), run, &session.workspace, session.owner_epoch)?;
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
        (agent_decision::Tool::WriteFile, "workspace.write.approval"),
      ]
      .into_iter()
      .filter(|(tool, _)| plan.allowed_tools.contains(tool))
      .map(|(_, capability)| capability.to_owned())
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
      permitted_tools.is_none_or(|permitted| permitted.contains(&tool.definition().name))
        && plan
          .allowed_tools
          .iter()
          .any(|allowed| allowed.name() == tool.definition().name)
    });
    let mut runtime_config = self.config.runtime(session);
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
        Arc::new(self.store.clone()),
        tools,
        listener,
        backend,
      )
      .await?
    } else {
      Runtime::open(
        runtime_config,
        Arc::new(self.store.clone()),
        tools,
        listener,
      )
      .await?
    };
    let execute = runtime.execute(&run.prompt);
    tokio::pin!(execute);
    let result = tokio::select! {
      biased;
      _=cancel.cancelled()=>{runtime.cancel().await?;tokio::time::timeout(Duration::from_secs(10),&mut execute).await.map_err(|_|RuntimeError::Uncertain("cancellation did not reach a durable terminal result".into()))?},
      result=&mut execute=>result,
      _=tokio::time::sleep(Duration::from_secs(self.config.run_timeout_secs))=>{runtime.cancel().await?;tokio::time::timeout(Duration::from_secs(10),&mut execute).await.map_err(|_|RuntimeError::Uncertain("execution deadline expired with unknown outcome".into()))?},
    };
    if let Some(computer) = &computer {
      computer.close().await;
    }
    let result = result?;
    if persist_failed.load(Ordering::Acquire) {
      return Err(
        RuntimeError::Uncertain(
          "runtime event persistence failed; execution requires reconciliation".into(),
        )
        .into(),
      );
    }
    let (status, error) = match result.outcome {
      RunOutcome::Completed => (RunStatus::Completed, String::new()),
      RunOutcome::Cancelled => (RunStatus::Cancelled, String::new()),
      RunOutcome::Failed { message } => (RunStatus::Failed, message),
      RunOutcome::Uncertain { message } => (RunStatus::Uncertain, message),
      RunOutcome::Suspended { id } => (
        RunStatus::Uncertain,
        format!("harness suspended operation {id}; explicit reconciliation required"),
      ),
    };
    self
      .store
      .update_run_owned(&run.id, session.owner_epoch, status, &result.text, &error)?;
    self.events.notify_waiters();
    Ok(())
  }
}
