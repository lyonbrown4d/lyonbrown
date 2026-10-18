use std::collections::BTreeMap;

use agent_config::{RelaySettings, load_with_environment};

#[test]
fn relay_is_opt_in_and_requires_the_worker_runtime() {
  let data = tempfile::tempdir().unwrap();
  assert!(
    load_with_environment(data.path(), None, &BTreeMap::new())
      .unwrap()
      .relay
      .is_none()
  );
  let path = data.path().join("config.toml");
  let relay = "[relay]\ncontrol_node_id = 'control'\nendpoint = 'https://relay.example'\nroutes = ['worker']\n";
  std::fs::write(&path, relay).unwrap();
  assert!(load_with_environment(data.path(), Some(&path), &BTreeMap::new()).is_err());
  std::fs::write(
    &path,
    format!(
      "{relay}\n[peer]\nlisten = '127.0.0.1:7448'\nadvertise_url = 'https://worker.example:7448'\n"
    ),
  )
  .unwrap();
  let settings = load_with_environment(data.path(), Some(&path), &BTreeMap::new()).unwrap();
  assert!(settings.relay.unwrap().routes.contains("worker"));
}

#[test]
fn relay_rejects_credential_origins_and_unbounded_or_control_routes() {
  let valid = RelaySettings {
    control_node_id: "control".into(),
    endpoint: "https://relay.example".into(),
    routes: ["worker".into()].into(),
  };
  for endpoint in [
    "http://relay.example",
    "https://relay.example/path",
    "https://user:synthetic-secret@relay.example",
    "https://relay.example?token=synthetic-secret",
    "https://relay.example#secret",
  ] {
    let error = RelaySettings {
      endpoint: endpoint.into(),
      ..valid.clone()
    }
    .validate()
    .unwrap_err();
    assert!(!error.to_string().contains("synthetic-secret"));
  }
  for routes in [
    Default::default(),
    ["control".into()].into(),
    ["bad\nnode".into()].into(),
    (0..33).map(|index| format!("worker-{index}")).collect(),
  ] {
    assert!(
      RelaySettings {
        routes,
        ..valid.clone()
      }
      .validate()
      .is_err()
    );
  }
}
