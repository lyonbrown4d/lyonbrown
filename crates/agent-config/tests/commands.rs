use agent_config::load_with_environment;
use std::collections::BTreeMap;

#[test]
fn commands_default_disabled_and_resolve_only_explicit_private_environment_references() {
  let directory = tempfile::tempdir().unwrap();
  let defaults = load_with_environment(directory.path(), None, &BTreeMap::new()).unwrap();
  assert!(defaults.commands.executables.is_empty());
  let path = directory.path().join("config.toml");
  std::fs::write(
    &path,
    "[commands.environment]\nSELECTED = \"CHILD_SECRET\"\n",
  )
  .unwrap();
  std::fs::write(
    directory.path().join(".env"),
    "CHILD_SECRET=dotenv-private\nUNSELECTED=extra-private\n",
  )
  .unwrap();
  let settings = load_with_environment(
    directory.path(),
    Some(&path),
    &BTreeMap::from([("CHILD_SECRET".into(), "environment-private".into())]),
  )
  .unwrap();
  assert_eq!(
    settings.command_environment_value("CHILD_SECRET"),
    Some("environment-private")
  );
  assert_eq!(settings.command_environment_value("UNSELECTED"), None);
  let debug = format!("{settings:?}");
  assert!(!debug.contains("environment-private"));
  assert!(!debug.contains("dotenv-private"));
  assert!(!debug.contains("extra-private"));
}

#[test]
fn invalid_or_missing_command_environment_references_fail_without_exposing_values() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("config.toml");
  for body in [
    "[commands]\nexecutables = ['relative-program']\n",
    "[commands.environment]\nSELECTED = 'MISSING'\n",
    "[commands.environment]\nSELECTED = 'literal-secret-value'\n",
  ] {
    std::fs::write(&path, body).unwrap();
    let error = load_with_environment(directory.path(), Some(&path), &BTreeMap::new()).unwrap_err();
    assert!(!error.to_string().contains("literal-secret-value"));
  }
}
