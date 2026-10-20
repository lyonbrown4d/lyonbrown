use super::{GatewayPlan, PreparedToolchain, absolute, process};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
  collections::BTreeMap,
  ffi::{OsStr, OsString},
  path::{Component, Path, PathBuf},
  process::Command,
};

pub(super) async fn build(
  plan: &mut GatewayPlan,
  toolchain: &PreparedToolchain,
  leptos: &Path,
  ambient: &BTreeMap<OsString, OsString>,
) -> Result<PathBuf> {
  let mut metadata = toolchain.cargo_command(&plan.root, ambient)?;
  metadata.args(["metadata", "--locked", "--no-deps", "--format-version=1"]);
  let output = process::output(metadata).await?;
  let metadata: cargo_metadata::Metadata =
    serde_json::from_slice(&output.stdout).context("read Cargo workspace metadata")?;
  let project = metadata.workspace_metadata["leptos"]
    .as_array()
    .and_then(|projects| projects.iter().find(|project| project["name"] == "gateway"))
    .context("workspace gateway cargo-leptos project is missing")?;
  ensure!(
    project["bin-package"] == "ai-gateway"
      && project["lib-package"] == "gateway-web"
      && project["output-name"] == "gateway_web"
      && project["site-pkg-dir"] == "ui/pkg",
    "workspace gateway build metadata does not match the Gateway asset contract"
  );
  let site_root = Path::new(
    project["site-root"]
      .as_str()
      .context("gateway site-root is required")?,
  );
  ensure!(
    !site_root.is_absolute()
      && !site_root.as_os_str().is_empty()
      && site_root
        .components()
        .all(|component| matches!(component, Component::Normal(_))),
    "gateway site-root must be a repository-relative directory without parent traversal"
  );
  plan.web_root = absolute(&plan.root, site_root);
  let mut build = toolchain.command(leptos, &plan.root, ambient)?;
  apply_environment(&mut build, plan, project, ambient);
  build.args(["leptos", "build", "--project", "gateway"]);
  if plan.release {
    build.arg("--release");
  }
  process::status(build).await?;
  // Cargo's artifact message honors target-dir, build.target, profiles and platform suffixes.
  let command = server_build_command(plan, toolchain, project, ambient)?;
  process::executable(command).await
}

pub(super) fn server_build_command(
  plan: &GatewayPlan,
  toolchain: &PreparedToolchain,
  project: &Value,
  ambient: &BTreeMap<OsString, OsString>,
) -> Result<Command> {
  let mut command = toolchain.cargo_command(&plan.root, ambient)?;
  apply_environment(&mut command, plan, project, ambient);
  command.args([
    "build",
    "--locked",
    "-p",
    "ai-gateway",
    "--bin",
    "ai-gateway",
    "--message-format=json-render-diagnostics",
  ]);
  if project["bin-default-features"].as_bool() != Some(true) {
    command.arg("--no-default-features");
  }
  let features = project["bin-features"]
    .as_array()
    .map(|features| {
      features
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(",")
    })
    .unwrap_or_else(|| "ssr".into());
  if !features.is_empty() {
    command.args(["--features", &features]);
  }
  let profile_key = if plan.release {
    "bin-profile-release"
  } else {
    "bin-profile-dev"
  };
  let profile =
    project[profile_key]
      .as_str()
      .unwrap_or(if plan.release { "release" } else { "dev" });
  command.args([
    "--profile",
    if profile == "debug" { "dev" } else { profile },
  ]);
  if let Some(target) = ambient.get(OsStr::new("LEPTOS_BIN_TARGET_TRIPLE")) {
    command.arg("--target").arg(target);
  } else if let Some(target) = project["bin-target-triple"].as_str() {
    command.args(["--target", target]);
  }
  if let Some(args) = project["bin-cargo-args"].as_array() {
    for arg in args {
      command.arg(
        arg
          .as_str()
          .context("gateway bin-cargo-args must be strings")?,
      );
    }
  }
  Ok(command)
}
fn apply_environment(
  command: &mut Command,
  plan: &GatewayPlan,
  project: &Value,
  ambient: &BTreeMap<OsString, OsString>,
) {
  command
    .env("LEPTOS_OUTPUT_NAME", "gateway_web")
    .env("LEPTOS_SITE_PKG_DIR", "ui/pkg")
    .env("LEPTOS_SITE_ROOT", &plan.web_root)
    .env("LEPTOS_HASH_FILES", "false");
  if let Some(target) = ambient.get(OsStr::new("LEPTOS_BIN_TARGET_TRIPLE")) {
    command.env("LEPTOS_BIN_TARGET_TRIPLE", target);
  }
  for key in ["LEPTOS_WASM_SPLIT", "LEPTOS_WASM_OPT"] {
    if let Some(value) = ambient.get(OsStr::new(key)) {
      command.env(key, value);
    }
  }
  if let Some(address) = project["site-addr"].as_str() {
    command.env("LEPTOS_SITE_ADDR", address);
  }
}
