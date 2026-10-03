use agent_model::{ModelError, RegistryError};
use agentd::runtime::catalog_failure_is_fatal;

#[test]
fn operational_catalog_failures_degrade_to_an_absent_catalog() {
  assert!(!catalog_failure_is_fatal(&RegistryError::CatalogTimeout(
    "provider".into(),
  )));
  assert!(!catalog_failure_is_fatal(&RegistryError::Provider(
    ModelError::Other("operational".into()),
  )));
  assert!(!catalog_failure_is_fatal(&RegistryError::NoModels));
}

#[test]
fn invariant_catalog_failures_abort_runtime_bootstrap() {
  assert!(catalog_failure_is_fatal(&RegistryError::ProviderIdentity {
    registered: "one".into(),
    declared: "two".into(),
  }));
  assert!(catalog_failure_is_fatal(&RegistryError::DuplicateProvider(
    "provider".into(),
  )));
  assert!(catalog_failure_is_fatal(&RegistryError::DuplicateRoute));
  assert!(catalog_failure_is_fatal(&RegistryError::InvalidRoute));
  assert!(catalog_failure_is_fatal(&RegistryError::CatalogOverflow(
    129
  )));
}
