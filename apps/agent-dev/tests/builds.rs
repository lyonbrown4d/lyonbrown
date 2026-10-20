use agent_dev::{Options, Plan, ProductBuild, Target};
use anyhow::{Context, Result};
use clap::Parser;
use std::{collections::BTreeMap, ffi::OsStr, process::Command};

#[test]
fn product_builds_pass_exact_cargo_arguments_and_features_only_to_clients() -> Result<()> {
  let root = tempfile::tempdir()?;
  for (target, package) in [
    ("daemon", "agentd"),
    ("desktop", "agent-desktop"),
    ("tui", "agent-cli"),
  ] {
    for native in [false, true] {
      if target == "daemon" && native {
        continue;
      }
      let mut arguments = vec!["agent-dev", target];
      if native {
        arguments.push("--native-computer");
      }
      let options = Options::try_parse_from(arguments)?;
      let plan = Plan::new(root.path(), &options)?;
      let mut expected = vec![
        "build",
        "--locked",
        "-p",
        package,
        "--bin",
        package,
        "--no-default-features",
      ];
      if native {
        expected.extend(["--features", "native-computer"]);
      }
      expected.push("--message-format=json-render-diagnostics");
      let command = plan.product_build().command(root.path());
      assert_eq!(command.get_current_dir(), Some(root.path()));
      let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
      assert_eq!(command.get_program(), cargo.as_os_str());

      assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        expected.iter().map(OsStr::new).collect::<Vec<_>>()
      );
      let daemon = ProductBuild::Daemon.command(root.path());
      assert_eq!(
        daemon.get_args().collect::<Vec<_>>(),
        [
          "build",
          "--locked",
          "-p",
          "agentd",
          "--bin",
          "agentd",
          "--no-default-features",
          "--message-format=json-render-diagnostics",
        ]
        .map(OsStr::new)
      );
      let default = Options::try_parse_from(["agent-dev", target])?;
      let default = Plan::new(root.path(), &default)?;
      assert_eq!(
        plan.environment(&BTreeMap::new()),
        default.environment(&BTreeMap::new()),
        "build opt-in must not change daemon configuration or tool permissions"
      );
    }
  }
  Ok(())
}

#[test]
fn dry_run_reports_client_feature_build_without_running_cargo_or_preparing_data() -> Result<()> {
  let root = tempfile::tempdir()?;
  for (target, package) in [("desktop", "agent-desktop"), ("tui", "agent-cli")] {
    for native in [false, true] {
      let data = root.path().join(format!("{target}-{native}"));
      let mut command = Command::new(env!("CARGO_BIN_EXE_agent-dev"));
      command
        .args([target, "--dry-run", "--data-dir"])
        .arg(&data)
        .env("CARGO", root.path().join("cargo-must-not-be-executed"));
      if native {
        command.arg("--native-computer");
      }
      let output = command.output()?;
      let stderr = String::from_utf8(output.stderr)?;
      assert!(output.status.success(), "{stderr}");
      let mut expected =
        format!("cargo build --locked -p {package} --bin {package} --no-default-features");
      if native {
        expected.push_str(" --features native-computer");
      }
      assert_eq!(
        stderr
          .lines()
          .filter(|line| line.starts_with("cargo "))
          .collect::<Vec<_>>(),
        [
          "cargo build --locked -p agentd --bin agentd --no-default-features",
          &expected,
        ]
      );
      assert!(!data.exists());
    }
  }
  Ok(())
}

#[test]
fn daemon_rejects_native_computer_before_preparation_or_building() -> Result<()> {
  let root = tempfile::tempdir()?;
  let options = Options::try_parse_from(["agent-dev", "daemon", "--native-computer"])?;
  assert_eq!(options.target, Target::Daemon);
  let error = Plan::new(root.path(), &options)
    .err()
    .context("daemon must reject a client-only feature")?;
  assert!(
    error
      .to_string()
      .contains("--native-computer requires desktop or tui")
  );
  let output = Command::new(env!("CARGO_BIN_EXE_agent-dev"))
    .args(["daemon", "--native-computer", "--dry-run", "--data-dir"])
    .arg(root.path().join("unused"))
    .env("CARGO", root.path().join("cargo-must-not-be-executed"))
    .output()?;
  assert!(!output.status.success());
  let stderr = String::from_utf8(output.stderr)?;
  assert!(stderr.contains("--native-computer requires desktop or tui"));
  assert!(!stderr.contains("cargo build"));
  assert!(!root.path().join("unused").exists());
  assert!(!root.path().join(".local").exists());
  Ok(())
}

#[cfg(unix)]
#[test]
fn cargo_diagnostics_remain_visible_when_the_build_fails() -> Result<()> {
  use std::os::unix::fs::PermissionsExt;
  let root = tempfile::tempdir()?;
  let diagnostic = serde_json::json!({
    "reason": "compiler-message",
    "package_id": "path+file:///fixture#0.1.0",
    "target": { "name": "agentd", "kind": ["bin"], "src_path": "/fixture/main.rs" },
    "message": {
      "message": "fixture compiler error", "code": null, "level": "error",
      "spans": [], "children": [], "rendered": "fixture rendered compiler diagnostic\n"
    }
  });
  let messages = root.path().join("diagnostic.jsonl");
  std::fs::write(&messages, diagnostic.to_string())?;
  let cargo = root.path().join("cargo-fixture");
  std::fs::write(
    &cargo,
    "#!/bin/sh\ncat \"$FIXTURE_MESSAGES\"\nprintf '\\n'\nexit 7\n",
  )?;
  std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o700))?;
  let output = Command::new(env!("CARGO_BIN_EXE_agent-dev"))
    .args(["daemon", "--data-dir"])
    .arg(root.path().join("data"))
    .env("CARGO", cargo)
    .env("FIXTURE_MESSAGES", messages)
    .output()?;
  assert!(!output.status.success());
  let stderr = String::from_utf8(output.stderr)?;
  assert!(
    stderr.contains("fixture rendered compiler diagnostic"),
    "{stderr}"
  );
  assert!(stderr.contains("agentd build failed"), "{stderr}");
  Ok(())
}
