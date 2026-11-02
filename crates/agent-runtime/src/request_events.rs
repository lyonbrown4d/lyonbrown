//! The SDK hook runs immediately before each provider request, including
//! later turns. RunStarted only describes Harness preparation.
use crate::{EventListener, RuntimeEvent};
use rpi_ai::{Context, Model, ProviderHooks, SimpleStreamOptions, SimpleStreamOptionsPatch};

pub(crate) struct RequestEvents(pub EventListener);

impl ProviderHooks for RequestEvents {
  fn before_request(
    &self,
    _model: &Model,
    _context: &Context,
    _options: &SimpleStreamOptions,
  ) -> Option<SimpleStreamOptionsPatch> {
    (self.0)(RuntimeEvent::ModelRequestStarted);
    None
  }
}
