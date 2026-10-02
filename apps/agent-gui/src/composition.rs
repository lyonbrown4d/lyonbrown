use std::{error::Error, fmt};

use agent_client::NativeAgentClient;
use fluxdi::{Injector, Module, Provider, Shared};

use crate::{app::Cockpit, state::CockpitState};

#[derive(Debug)]
pub struct CompositionError(String);

impl fmt::Display for CompositionError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

impl Error for CompositionError {}

struct GuiModule {
  initial_state: CockpitState,
  native_client: Option<NativeAgentClient>,
}

impl Module for GuiModule {
  fn providers(&self, injector: &Injector) {
    let initial_state = self.initial_state.clone();
    let native_client = self.native_client.clone();
    injector.provide::<CockpitState>(Provider::root(move |_| Shared::new(initial_state.clone())));
    injector.provide::<CockpitFactory>(
      Provider::root(move |injector| {
        Shared::new(CockpitFactory {
          initial_state: injector.resolve(),
          native_client: native_client.clone(),
        })
      })
      .with_dependency::<CockpitState>(),
    );
  }
}

struct CockpitFactory {
  initial_state: Shared<CockpitState>,
  native_client: Option<NativeAgentClient>,
}

impl CockpitFactory {
  fn create(&self) -> Cockpit {
    Cockpit::new(
      self.initial_state.as_ref().clone(),
      self.native_client.clone(),
    )
  }
}

pub struct CompositionRoot {
  injector: Injector,
}

impl CompositionRoot {
  pub fn with_native_client(native_client: NativeAgentClient) -> Result<Self, CompositionError> {
    Self::build(CockpitState::default(), Some(native_client))
  }

  pub fn unavailable(error: impl Into<String>) -> Result<Self, CompositionError> {
    let mut state = CockpitState::default();
    state.apply_connection(
      crate::state::ConnectionState::Unavailable,
      Some(error.into()),
    );
    Self::build(state, None)
  }

  fn build(
    initial_state: CockpitState,
    native_client: Option<NativeAgentClient>,
  ) -> Result<Self, CompositionError> {
    let injector = Injector::root();
    GuiModule {
      initial_state,
      native_client,
    }
    .configure(&injector)
    .map_err(|error| CompositionError(error.to_string()))?;
    let root = Self { injector };
    root.validate()?;
    Ok(root)
  }

  pub fn validate(&self) -> Result<(), CompositionError> {
    self
      .injector
      .try_validate_graph()
      .map_err(|error| CompositionError(error.to_string()))
  }

  pub fn create_cockpit(&self) -> Result<Cockpit, CompositionError> {
    let factory = self
      .injector
      .try_resolve::<CockpitFactory>()
      .map_err(|error| CompositionError(error.to_string()))?;
    Ok(factory.create())
  }
}
