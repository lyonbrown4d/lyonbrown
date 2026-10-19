//! Typed application composition; HTTP request scopes remain at the transport boundary.
use decision_core::DecisionEngine;
use fluxdi::{Error, Shared, static_module};
use gateway_core::{Gateway, GatewayConfig};
use gateway_storage::GatewayStorage;

static_module! {
  pub(crate) struct GatewayModule {
    #[scope(root)]
    gateway: Gateway = gateway(config, decision, storage);
    external config: GatewayConfig;
    external decision: DecisionEngine;
    external storage: GatewayStorage;
  }
}
fn gateway(
  config: Shared<GatewayConfig>,
  decision: Shared<DecisionEngine>,
  storage: Shared<GatewayStorage>,
) -> Result<Shared<Gateway>, Error> {
  Gateway::new_with_access((*config).clone(), decision, storage.clone(), storage)
    .map(Shared::new)
    .map_err(|_| Error::factory_failed("Gateway", "validated gateway assembly failed"))
}
