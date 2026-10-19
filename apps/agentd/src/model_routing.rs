//! The shared advisory engine is always embedded; model credentials stay outside it.
use crate::Daemon;
use agent_core::Run;
use decision_core::{
  DecisionCancellation, DecisionOutput, DecisionRequest, DecisionStatus, ModelCandidate, ModelTier,
  Operation, RouteInput,
};
use tokio_util::sync::CancellationToken;

impl Daemon {
  pub(crate) async fn record_model_route(
    &self,
    run: &Run,
    epoch: u64,
    cancellation: &CancellationToken,
  ) -> anyhow::Result<()> {
    let local = self.config.provider == "local";
    let context_tokens = if local {
      Some(u64::from(
        self.models.info(&self.config.model)?.context_window,
      ))
    } else {
      None
    };
    // An explicitly configured main model remains pinned for this run. The engine
    // cannot replace it with an unconfigured provider or a merely discovered model.
    let mut request = DecisionRequest::new(
      &run.id,
      Operation::RouteModel(RouteInput {
        text: run.prompt.clone(),
        candidates: vec![ModelCandidate {
          key: self.config.model.clone(),
          tier: ModelTier::Balanced,
          local,
          capabilities: Default::default(),
          context_tokens,
          estimated_cost_microusd: None,
        }],
        explicit_model: None,
        pinned_model: Some(self.config.model.clone()),
      }),
    );
    request.deadline_ms = self.config.decision.timeout_ms;
    let token = DecisionCancellation::new();
    let engine = self.decision.reusable();
    let result = tokio::select! {
      biased;
      _=cancellation.cancelled()=>{token.cancel();return Ok(());},
      result=engine.decide(&request,&token)=>result,
    };
    self.store.append_event_owned(
      &run.id,
      epoch,
      "model_route_decision",
      serde_json::to_value(&result)?,
    )?;
    anyhow::ensure!(
      result.status != DecisionStatus::Denied,
      "shared decision policy denied the configured model route"
    );
    anyhow::ensure!(
      matches!(result.output, DecisionOutput::RouteModel(ref route) if route.candidate_model_keys.iter().any(|key|key==&self.config.model)),
      "shared decision engine produced no eligible configured model route"
    );
    Ok(())
  }
}
