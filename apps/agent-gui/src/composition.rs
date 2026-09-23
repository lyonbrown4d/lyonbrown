use std::{error::Error, fmt};

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

struct GuiModule;

impl Module for GuiModule {
  fn providers(&self, injector: &Injector) {
    injector.provide::<CockpitState>(Provider::root(|_| Shared::default()));
    injector.provide::<CockpitFactory>(
      Provider::root(|injector| {
        Shared::new(CockpitFactory {
          initial_state: injector.resolve(),
        })
      })
      .with_dependency::<CockpitState>(),
    );
  }
}

struct CockpitFactory {
  initial_state: Shared<CockpitState>,
}

impl CockpitFactory {
  fn create(&self) -> Cockpit {
    Cockpit::new(self.initial_state.as_ref().clone())
  }
}

pub struct CompositionRoot {
  injector: Injector,
}

impl CompositionRoot {
  pub fn new() -> Result<Self, CompositionError> {
    let injector = Injector::root();
    GuiModule
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
