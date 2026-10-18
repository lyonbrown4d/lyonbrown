use agent_dev::{Options, Plan, Target};
use anyhow::Result;
use std::{collections::BTreeMap, path::PathBuf};

fn options(target: Target) -> Options {
  Options {
    target,
    data_dir: None,
    config: None,
    listen: "127.0.0.1:7547".parse().expect("fixture socket"),
    dry_run: false,
  }
}
#[test]
fn explicit_development_environment_discards_ambient_production_connection() -> Result<()> {
  let root = tempfile::tempdir()?;
  let mut plan = Plan::new(root.path(), &options(Target::Tui))?;
  let ambient = BTreeMap::from([
    ("LYONBROWN_TOKEN".into(), "production-secret".into()),
    ("LYONBROWN_DEV_READY_NONCE".into(), "ambient-nonce".into()),
    (
      "LYONBROWN_ENDPOINT".into(),
      "https://example.invalid".into(),
    ),
    ("LYONBROWN_CONFIG".into(), "/production/config.toml".into()),
    ("LYONBROWN_LISTEN".into(), "127.0.0.1:7447".into()),
    ("LYONBROWN_DATA_DIR".into(), "/production/data".into()),
  ]);
  plan.prepare(&ambient)?;
  let environment = plan.environment(&ambient);
  assert!(!environment.contains_key("LYONBROWN_TOKEN"));
  assert!(!environment.contains_key("LYONBROWN_CONFIG"));
  assert!(!environment.contains_key("LYONBROWN_DEV_READY_NONCE"));
  assert_eq!(environment["LYONBROWN_ENDPOINT"], "http://127.0.0.1:7547");
  assert_eq!(environment["LYONBROWN_LISTEN"], "127.0.0.1:7547");
  assert_eq!(
    PathBuf::from(&environment["LYONBROWN_DATA_DIR"]),
    plan.data_dir
  );
  Ok(())
}
#[test]
fn rejects_nonloopback_zero_and_production_ports_before_creating_data() -> Result<()> {
  let root = tempfile::tempdir()?;
  for socket in ["0.0.0.0:7547", "127.0.0.1:0", "127.0.0.1:7447"] {
    let mut options = options(Target::Daemon);
    options.listen = socket.parse()?;
    assert!(Plan::new(root.path(), &options).is_err());
  }
  assert!(!root.path().join(".local").exists());
  Ok(())
}
#[test]
fn custom_config_and_adjacent_dotenv_are_shared_without_rewriting() -> Result<()> {
  let root = tempfile::tempdir()?;
  let config = root.path().join("custom.toml");
  let contents = "model = 'development-model'\n";
  std::fs::write(&config, contents)?;
  std::fs::write(
    root.path().join(".env"),
    "LYONBROWN_LOG_DIRECTORY=dev-logs\n",
  )?;
  let mut options = options(Target::Desktop);
  options.data_dir = Some(PathBuf::from("data with spaces"));
  options.config = Some(PathBuf::from("custom.toml"));
  let mut plan = Plan::new(root.path(), &options)?;
  plan.prepare(&BTreeMap::new())?;
  let settings = agent_config::load_with_environment(
    &plan.data_dir,
    plan.config.as_deref(),
    &plan.environment(&BTreeMap::new()),
  )?;
  assert_eq!(settings.model, "development-model");
  assert_eq!(settings.log_directory, plan.data_dir.join("dev-logs"));
  assert_eq!(std::fs::read_to_string(config)?, contents);
  Ok(())
}
