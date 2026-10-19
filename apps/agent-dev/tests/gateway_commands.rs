use agent_dev::gateway::{CARGO_LEPTOS_VERSION, GatewayOptions, GatewayPlan, tool_command};
use clap::Parser;
use std::{
  collections::BTreeMap,
  ffi::{OsStr, OsString},
  path::Path,
  process::Command,
};

#[test]
fn gateway_help_and_dry_run_do_not_execute_tools_or_read_credentials() {
  let directory = tempfile::tempdir().expect("tempdir");
  let data = directory.path().join("data with spaces");
  let config = directory.path().join("missing config.toml");
  for arguments in [
    vec!["--help"],
    vec!["--dry-run", "--build-only", "--release"],
  ] {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gateway-dev"));
    command
      .args(&arguments)
      .env("CARGO", directory.path().join("must-not-execute"))
      .env(
        "LYONBROWN_GATEWAY_ADMIN_PASSWORD",
        "private-launcher-password",
      );
    if arguments[0] != "--help" {
      command
        .arg("--config")
        .arg(&config)
        .arg("--data-dir")
        .arg(&data);
    }
    let output = command.output().expect("gateway-dev process");
    assert!(output.status.success());
    let output = format!(
      "{}{}",
      String::from_utf8_lossy(&output.stdout),
      String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.contains("private-launcher-password"));
    if arguments[0] == "--help" {
      assert!(output.contains("--build-only"));
    } else {
      assert!(output.contains("leptos build --project gateway --release"));
      assert!(output.contains(CARGO_LEPTOS_VERSION));
      assert!(!output.contains("agentd"));
    }
    assert!(!data.exists());
    assert!(!config.exists());
  }
}
#[test]
fn build_tools_preserve_target_configuration_without_gateway_credentials() {
  let directory = tempfile::tempdir().expect("tempdir");
  let ambient = BTreeMap::from([
    (OsString::from("PATH"), OsString::from("toolchain-bin")),
    ("CARGO_TARGET_DIR".into(), "target with spaces".into()),
    (
      "CARGO_BUILD_TARGET".into(),
      "x86_64-unknown-linux-gnu".into(),
    ),
    ("CARGO_PROFILE_DEV_DEBUG".into(), "0".into()),
    ("RUSTUP_TOOLCHAIN".into(), "1.99.1".into()),
    (
      "LYONBROWN_GATEWAY_ADMIN_PASSWORD".into(),
      "private-password".into(),
    ),
    ("LYONBROWN_GATEWAY_TOKEN".into(), "private-api-token".into()),
    ("CUSTOM_PROVIDER_KEY".into(), "private-provider-key".into()),
  ]);
  let command = tool_command(Path::new("cargo"), directory.path(), &ambient);
  let environment: BTreeMap<_, _> = command.get_envs().collect();
  for name in [
    "PATH",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_TARGET",
    "CARGO_PROFILE_DEV_DEBUG",
    "RUSTUP_TOOLCHAIN",
  ] {
    assert_eq!(
      environment[OsStr::new(name)],
      Some(ambient[OsStr::new(name)].as_os_str())
    );
  }
  for name in [
    "LYONBROWN_GATEWAY_ADMIN_PASSWORD",
    "LYONBROWN_GATEWAY_TOKEN",
    "CUSTOM_PROVIDER_KEY",
  ] {
    assert!(!environment.contains_key(OsStr::new(name)));
  }
  assert_eq!(command.get_current_dir(), Some(directory.path()));
}
#[test]
fn server_paths_are_separate_arguments_and_only_gateway_is_selected() {
  let directory = tempfile::tempdir().expect("tempdir");
  let options = GatewayOptions::try_parse_from([
    "gateway-dev",
    "--config",
    "config with spaces/gateway.toml",
    "--data-dir",
    "data with spaces",
    "--listen",
    "127.0.0.1:8199",
  ])
  .expect("options");
  let plan = GatewayPlan::new(directory.path(), &options).expect("plan");
  let executable = directory
    .path()
    .join(format!("ai-gateway{}", std::env::consts::EXE_SUFFIX));
  let command = plan.server_command(&executable);
  assert_eq!(command.get_program(), executable.as_os_str());
  let arguments: Vec<_> = command.get_args().collect();
  assert_eq!(arguments[0], "--data-dir");
  assert_eq!(
    arguments[1],
    directory.path().join("data with spaces").as_os_str()
  );
  assert_eq!(arguments[2], "--config");
  assert_eq!(
    arguments[3],
    directory
      .path()
      .join("config with spaces/gateway.toml")
      .as_os_str()
  );
  assert_eq!(
    &arguments[4..],
    [OsStr::new("--listen"), OsStr::new("127.0.0.1:8199")]
  );
  let environment: BTreeMap<_, _> = command.get_envs().collect();
  assert_eq!(
    environment[OsStr::new("LYONBROWN_GATEWAY_WEB_ROOT")],
    Some(plan.web_root.as_os_str())
  );
  for listener in ["0.0.0.0:8199", "127.0.0.1:0"] {
    let options =
      GatewayOptions::try_parse_from(["gateway-dev", "--listen", listener]).expect("options");
    assert!(GatewayPlan::new(directory.path(), &options).is_err());
  }
}

#[test]
fn rustup_reported_compilers_and_cargo_override_ambient_homebrew_tools_for_all_build_children() {
  use agent_dev::gateway::PreparedToolchain;
  let directory = tempfile::tempdir().expect("tempdir");
  let bin = directory.path().join("rustup-reported/bin");
  let cargo = bin.join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
  let rustc = bin.join(format!("rustc{}", std::env::consts::EXE_SUFFIX));
  let rustdoc = bin.join(format!("rustdoc{}", std::env::consts::EXE_SUFFIX));
  let prepared = PreparedToolchain::new(cargo.clone(), rustc.clone(), rustdoc.clone())
    .expect("rustup-reported paths");
  let other_bin = directory.path().join("other-distribution/bin");
  let target_dir = directory.path().join("custom-target");
  let ambient = BTreeMap::from([
    (
      OsString::from("PATH"),
      std::env::join_paths([&other_bin]).expect("PATH"),
    ),
    ("CARGO".into(), other_bin.join("cargo").into_os_string()),
    ("RUSTC".into(), other_bin.join("rustc").into_os_string()),
    ("RUSTDOC".into(), other_bin.join("rustdoc").into_os_string()),
    (
      "CARGO_TARGET_DIR".into(),
      target_dir.clone().into_os_string(),
    ),
    ("RUSTC_WRAPPER".into(), "configured-compiler-cache".into()),
    (
      "LYONBROWN_GATEWAY_ADMIN_PASSWORD".into(),
      "private-password".into(),
    ),
  ]);
  let leptos = directory
    .path()
    .join(format!("cargo-leptos{}", std::env::consts::EXE_SUFFIX));
  let cargo_child = prepared
    .cargo_command(directory.path(), &ambient)
    .expect("Cargo command");
  assert_eq!(cargo_child.get_program(), cargo.as_os_str());
  let leptos_child = prepared
    .command(&leptos, directory.path(), &ambient)
    .expect("Leptos command");
  assert_eq!(leptos_child.get_program(), leptos.as_os_str());
  for command in [cargo_child, leptos_child] {
    let environment: BTreeMap<_, _> = command.get_envs().collect();
    for (name, path) in [
      ("CARGO", &cargo),
      ("RUSTC", &rustc),
      ("RUSTDOC", &rustdoc),
      ("CARGO_TARGET_DIR", &target_dir),
    ] {
      assert_eq!(environment[OsStr::new(name)], Some(path.as_os_str()));
    }
    let child_path = environment[OsStr::new("PATH")].expect("child PATH");
    assert_eq!(
      std::env::split_paths(child_path).collect::<Vec<_>>(),
      [bin.clone(), other_bin.clone()]
    );
    assert_eq!(
      environment[OsStr::new("RUSTC_WRAPPER")],
      Some(OsStr::new("configured-compiler-cache"))
    );
    assert!(!environment.contains_key(OsStr::new("LYONBROWN_GATEWAY_ADMIN_PASSWORD")));
  }
  assert_eq!(
    ambient[OsStr::new("RUSTC")],
    other_bin.join("rustc").as_os_str()
  );
}
