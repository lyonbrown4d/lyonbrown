use std::{collections::BTreeMap, fs};

use agent_config::load_with_environment;
use anyhow::Result;

#[test]
fn computer_tools_default_disabled_and_follow_trusted_configuration_layers() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let environment = BTreeMap::new();
  assert!(!load_with_environment(directory.path(), None, &environment)?.computer_tools);
  assert_eq!(
    load_with_environment(directory.path(), None, &environment)?.computer_lease_secs,
    300
  );

  fs::write(
    directory.path().join("config.toml"),
    "computer_tools = true\n",
  )?;
  assert!(load_with_environment(directory.path(), None, &environment)?.computer_tools);

  fs::write(
    directory.path().join(".env"),
    "LYONBROWN_COMPUTER_TOOLS=false\n",
  )?;
  assert!(!load_with_environment(directory.path(), None, &environment)?.computer_tools);

  let environment = BTreeMap::from([("LYONBROWN_COMPUTER_TOOLS".into(), "true".into())]);
  assert!(load_with_environment(directory.path(), None, &environment)?.computer_tools);
  Ok(())
}

#[test]
fn computer_tools_reject_invalid_boolean_without_echoing_values() -> Result<()> {
  let directory = tempfile::tempdir()?;
  fs::write(
    directory.path().join("config.toml"),
    "computer_tools = 'invalid-private-value'\n",
  )?;
  let error = load_with_environment(directory.path(), None, &BTreeMap::new())
    .expect_err("invalid file boolean");
  assert!(!format!("{error:#}").contains("invalid-private-value"));

  fs::write(
    directory.path().join("config.toml"),
    "computer_tools = false\n",
  )?;
  let environment = BTreeMap::from([(
    "LYONBROWN_COMPUTER_TOOLS".into(),
    "invalid-private-value".into(),
  )]);
  let error = load_with_environment(directory.path(), None, &environment)
    .expect_err("invalid environment boolean");
  assert!(!format!("{error:#}").contains("invalid-private-value"));
  Ok(())
}

#[test]
fn computer_lease_is_bounded_and_can_be_shortened_for_development() -> Result<()> {
  let directory = tempfile::tempdir()?;
  fs::write(
    directory.path().join("config.toml"),
    "computer_lease_secs = 20\n",
  )?;
  fs::write(
    directory.path().join(".env"),
    "LYONBROWN_COMPUTER_LEASE_SECS=15\n",
  )?;
  assert_eq!(
    load_with_environment(directory.path(), None, &BTreeMap::new())?.computer_lease_secs,
    15
  );
  for seconds in [10, 300] {
    let env = BTreeMap::from([("LYONBROWN_COMPUTER_LEASE_SECS".into(), seconds.to_string())]);
    assert_eq!(
      load_with_environment(directory.path(), None, &env)?.computer_lease_secs,
      seconds
    );
  }
  for seconds in [0, 9, 301] {
    let env = BTreeMap::from([("LYONBROWN_COMPUTER_LEASE_SECS".into(), seconds.to_string())]);
    assert!(load_with_environment(directory.path(), None, &env).is_err());
  }
  Ok(())
}
