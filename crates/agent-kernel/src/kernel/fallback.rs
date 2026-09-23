use super::*;

impl Kernel {
  pub(super) async fn start_model(
    &mut self,
    turn_id: TurnId,
    step_id: StepId,
    prepared: PreparedStep,
  ) -> Result<(SchedulerSelection, ModelStream), AgentError> {
    let PreparedStep {
      mut selected,
      mut request,
      step_context,
    } = prepared;
    let cancellation = self.cancellation.clone();
    let initial = match tokio::select! {
        () = cancellation.cancelled() => {
            self.cancel_turn(turn_id)?;
            return Err(AgentError::Cancelled);
        }
        stream = selected.provider.stream(request.clone()) => stream,
    } {
      Ok(mut stream) => {
        let first = tokio::select! {
          () = cancellation.cancelled() => {
            self.cancel_turn(turn_id)?;
            return Err(AgentError::Cancelled);
          }
          event = stream.events.next() => event,
        };
        match first {
          Some(Ok(event)) => {
            stream.events =
              Box::pin(futures::stream::once(async move { Ok(event) }).chain(stream.events));
            Ok(stream)
          }
          Some(Err(error)) => Err(error),
          None => Err(ModelError::Protocol(
            "provider stream ended before first event".into(),
          )),
        }
      }
      Err(error) => Err(error),
    };
    let stream = match initial {
      Ok(stream) => stream,
      Err(error) => {
        let retryable = matches!(error, ModelError::Timeout | ModelError::Http(500..=599, _));
        let local_worker = selected.capabilities.locality == Locality::Local
          && selected.capabilities.tier == ModelTier::Worker;
        let fallback = if retryable && local_worker && self.scheduler.allows_remote_fallback() {
          let mut fallback_context = step_context.clone();
          fallback_context.provider = None;
          fallback_context.model = None;
          fallback_context.escalated = true;
          self
            .scheduler
            .decide_ranked(&fallback_context)
            .await
            .ok()
            .and_then(|candidates| {
              candidates.into_iter().find(|candidate| {
                candidate.capabilities.locality == Locality::Remote
                  && candidate.capabilities.tier == ModelTier::Frontier
              })
            })
        } else {
          None
        };
        let Some(next) = fallback else {
          let error = AgentError::Model(self.broker.redact(&error.to_string()));
          self.record_error(error.to_string())?;
          return Err(error);
        };
        self.emit(AgentEvent::ModelFallback {
          step_id,
          from_provider: selected.capabilities.provider.clone(),
          from_model: selected.capabilities.model.clone(),
          to_provider: next.capabilities.provider.clone(),
          to_model: next.capabilities.model.clone(),
          reason: self.broker.redact(&error.to_string()),
          ts: Utc::now(),
        })?;
        selected = next;
        request = self.build_model_request(
          &step_context.prompt,
          step_id,
          &selected,
          &step_context,
          "fallback after local model error",
        )?;
        match tokio::select! {
          () = cancellation.cancelled() => {
            self.cancel_turn(turn_id)?;
            return Err(AgentError::Cancelled);
          }
          stream = selected.provider.stream(request) => stream,
        } {
          Ok(stream) => stream,
          Err(error) => {
            let error = AgentError::Model(self.broker.redact(&error.to_string()));
            self.record_error(error.to_string())?;
            return Err(error);
          }
        }
      }
    };
    Ok((selected, stream))
  }
}
