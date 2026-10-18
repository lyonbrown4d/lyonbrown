use std::collections::BTreeMap;

use agent_config::{DelegationTargetSettings, load_with_environment, validate_delegation_targets};

#[test]
fn delegation_is_disabled_by_default_and_only_operator_configuration_selects_routes() {
  let data = tempfile::tempdir().unwrap();
  assert!(
    load_with_environment(data.path(), None, &BTreeMap::new())
      .unwrap()
      .delegation_targets
      .is_empty()
  );
  let path = data.path().join("config.toml");
  std::fs::write(&path, "[[delegation_targets]]\nnode_id = 'worker-1'\nendpoint = 'https://192.0.2.1:7448'\ncapabilities = ['workspace.read']\n").unwrap();
  let settings = load_with_environment(data.path(), Some(&path), &BTreeMap::new()).unwrap();
  assert_eq!(settings.delegation_targets[0].node_id, "worker-1");
  assert_eq!(
    settings.delegation_targets[0].capabilities,
    ["workspace.read".into()].into()
  );
}

#[test]
fn targets_reject_endpoint_credentials_paths_duplicate_ids_and_privilege_expansion() {
  let target = DelegationTargetSettings {
    node_id: "worker-1".into(),
    endpoint: "https://192.0.2.1:7448".into(),
    capabilities: ["workspace.read".into()].into(),
  };
  for endpoint in [
    "http://192.0.2.1:7448",
    "https://worker.example/path",
    "https://user:synthetic-secret@worker.example",
    "https://worker.example?secret=synthetic-secret",
  ] {
    let error = validate_delegation_targets(&[DelegationTargetSettings {
      endpoint: endpoint.into(),
      ..target.clone()
    }])
    .unwrap_err();
    assert!(!error.to_string().contains("synthetic-secret"));
  }
  assert!(validate_delegation_targets(&[target.clone(), target.clone()]).is_err());
  assert!(
    validate_delegation_targets(&[DelegationTargetSettings {
      capabilities: ["tasks.submit".into()].into(),
      ..target
    }])
    .is_err()
  );
}
