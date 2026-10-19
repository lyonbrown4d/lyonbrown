mod support;
use model_mock::{AppConfig, Cli, Protocol};
#[test]
fn explicit_configuration_and_loopback_boundary_are_enforced() {
  let config = AppConfig::load(Cli {
    scenario: Some(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/development.toml").into()),
    listen: Some("127.0.0.1:0".parse().unwrap()),
    data_dir: None,
  })
  .unwrap();
  assert_eq!(config.listen.port(), 0);
  assert!(
    config
      .scenarios
      .iter()
      .any(|scenario| scenario.model == "mock-chat")
  );
  let mut config = support::settings(Protocol::ChatCompletions, vec![support::text("synthetic")]);
  config.listen = "0.0.0.0:8100".parse().unwrap();
  assert!(config.validate().is_err());
  config.listen = "127.0.0.1:0".parse().unwrap();
  config.scenarios[0].replies.clear();
  assert!(config.validate().is_err());
}
#[test]
fn cli_help_exposes_synthetic_scope_and_configuration() {
  let output = std::process::Command::new(env!("CARGO_BIN_EXE_model-mock"))
    .arg("--help")
    .output()
    .unwrap();
  assert!(output.status.success());
  let help = String::from_utf8(output.stdout).unwrap();
  assert!(help.contains("--scenario"));
  assert!(help.contains("--listen"));
  assert!(help.contains("never calls a real model"));
}
