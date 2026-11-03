use std::{collections::BTreeMap, fs};

use agent_config::load_with_environment;

#[test]
fn gateway_is_private_by_default_and_resolves_dedicated_layered_credentials() -> anyhow::Result<()>
{
  let directory = tempfile::tempdir()?;
  let defaults = load_with_environment(directory.path(), None, &BTreeMap::new())?;
  assert!(defaults.gateway.http_listen.is_none());
  fs::write(
    directory.path().join("config.toml"),
    "[gateway]\nhttp_listen='127.0.0.1:7448'\nhttp_token_env='MODEL_ACCESS'\n",
  )?;
  fs::write(
    directory.path().join(".env"),
    "MODEL_ACCESS=synthetic-dotenv-key\n",
  )?;
  let environment = BTreeMap::from([("MODEL_ACCESS".into(), "synthetic-process-key".into())]);
  let settings = load_with_environment(directory.path(), None, &environment)?;
  assert_eq!(
    settings.gateway_credential("MODEL_ACCESS").as_deref(),
    Some("synthetic-process-key")
  );
  assert!(!format!("{settings:?}").contains("synthetic-process-key"));
  assert!(!format!("{settings:?}").contains("synthetic-dotenv-key"));
  Ok(())
}

#[test]
fn gateway_listener_and_groups_reject_unsafe_configuration() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  for content in [
    "[gateway]\nhttp_listen='0.0.0.0:7448'\nhttp_token_env='MODEL_ACCESS'\n",
    "[gateway]\nhttp_listen='127.0.0.1:7448'\n",
    "[[gateway.groups]]\nid='coding'\nmodels=['missing']\n",
    "[gateway]\nhttp_token_env='invalid-name'\n",
  ] {
    fs::write(directory.path().join("config.toml"), content)?;
    assert!(load_with_environment(directory.path(), None, &BTreeMap::new()).is_err());
  }
  Ok(())
}
