use std::{collections::BTreeMap, fs};

use agent_config::load_with_environment;
use anyhow::Result;

#[test]
fn file_dotenv_and_real_environment_have_explicit_precedence_and_private_credentials() -> Result<()>
{
  let temp = tempfile::tempdir()?;
  fs::write(
    temp.path().join("config.toml"),
    "model = 'from-file'\nmax_runs = 2\nlog_directory = 'events'\n",
  )?;
  fs::write(
    temp.path().join(".env"),
    "LYONBROWN_MODEL=from-dotenv\nOPENAI_API_KEY=test-private-dotenv\nANTHROPIC_API_KEY=test-private-anthropic\n",
  )?;
  let environment = BTreeMap::from([
    ("LYONBROWN_MODEL".into(), "from-real-env".into()),
    ("OPENAI_API_KEY".into(), "test-private-real-env".into()),
  ]);
  let settings = load_with_environment(temp.path(), None, &environment)?;
  assert_eq!(settings.model, "from-real-env");
  assert_eq!(settings.max_runs, 2);
  assert_eq!(settings.log_directory, temp.path().join("events"));
  assert_eq!(
    settings.credential("openai").as_deref(),
    Some("test-private-real-env")
  );
  assert_eq!(
    settings.credential("anthropic").as_deref(),
    Some("test-private-anthropic")
  );
  assert!(!format!("{settings:?}").contains("test-private"));
  let dotenv_only = load_with_environment(temp.path(), None, &BTreeMap::new())?;
  assert_eq!(dotenv_only.model, "from-dotenv");
  assert_eq!(
    dotenv_only.credential("openai").as_deref(),
    Some("test-private-dotenv")
  );
  Ok(())
}

#[test]
fn validation_accepts_ipv6_and_rejects_unsafe_or_unbounded_settings() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let base = BTreeMap::from([
    ("LYONBROWN_LISTEN".into(), "[::1]:7447".into()),
    ("LYONBROWN_ENDPOINT".into(), "http://[::1]:7447".into()),
  ]);
  assert!(load_with_environment(temp.path(), None, &base).is_ok());
  for (key, value) in [
    ("LYONBROWN_LISTEN", "0.0.0.0:7447"),
    ("LYONBROWN_MAX_RUNS", "0"),
    ("LYONBROWN_RUN_TIMEOUT_SECS", "86401"),
    ("LYONBROWN_LOG_RETENTION", "0"),
    (
      "LYONBROWN_ENDPOINT",
      "http://user:secret@example.com/?token=private",
    ),
  ] {
    let mut invalid = base.clone();
    invalid.insert(key.into(), value.into());
    let error = load_with_environment(temp.path(), None, &invalid).expect_err("invalid settings");
    assert!(!format!("{error:#}").contains("private"));
    assert!(!format!("{error:#}").contains("secret"));
  }
  Ok(())
}

#[test]
fn explicit_file_is_required_and_invalid_dotenv_never_echoes_secret() -> Result<()> {
  let temp = tempfile::tempdir()?;
  assert!(
    load_with_environment(
      temp.path(),
      Some(&temp.path().join("missing.toml")),
      &BTreeMap::new()
    )
    .is_err()
  );
  fs::write(
    temp.path().join(".env"),
    "OPENAI_API_KEY='test-private-unclosed\n",
  )?;
  let error =
    load_with_environment(temp.path(), None, &BTreeMap::new()).expect_err("invalid dotenv");
  assert!(!format!("{error:#}").contains("test-private"));
  Ok(())
}

#[test]
fn decision_model_configuration_preserves_layering_and_validates_budget() -> Result<()> {
  let temp = tempfile::tempdir()?;
  fs::write(
    temp.path().join("config.toml"),
    "[decision]\nmodel='file-classifier'\ntimeout_ms=1500\nminimum_confidence=0.8\n",
  )?;
  fs::write(
    temp.path().join(".env"),
    "LYONBROWN_DECISION_MODEL=dotenv-classifier\n",
  )?;
  let environment = BTreeMap::from([("LYONBROWN_DECISION_MODEL".into(), "env-classifier".into())]);
  let settings = load_with_environment(temp.path(), None, &environment)?;
  assert_eq!(settings.decision.model.as_deref(), Some("env-classifier"));
  assert_eq!(settings.decision.timeout_ms, 1500);
  assert_eq!(settings.decision.minimum_confidence, 0.8);
  for (key, value) in [
    ("LYONBROWN_DECISION_TIMEOUT_MS", "0"),
    ("LYONBROWN_DECISION_MINIMUM_CONFIDENCE", "1.1"),
  ] {
    let mut invalid = environment.clone();
    invalid.insert(key.into(), value.into());
    assert!(load_with_environment(temp.path(), None, &invalid).is_err());
  }
  Ok(())
}
