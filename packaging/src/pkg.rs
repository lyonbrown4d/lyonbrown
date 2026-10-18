//! Immutable native client payloads with separately owned public selectors.
use std::{fs, os::unix::fs::symlink, path::Path, process::Command};

use agent_launch::bundle_identity::bundle_identity;
use anyhow::{Context, Result, ensure};
use semver::Version;

use crate::{copy_executable, run};

#[path = "pkg_components.rs"]
mod components;
use components::Component;

#[derive(Clone, Copy)]
enum Client {
  Cli,
  Desktop,
}

impl Client {
  fn name(self) -> &'static str {
    match self {
      Self::Cli => "cli",
      Self::Desktop => "desktop",
    }
  }

  fn bundle(self) -> &'static str {
    match self {
      Self::Cli => "Lyonbrown CLI.app",
      Self::Desktop => "Lyonbrown.app",
    }
  }

  fn selector(self) -> &'static str {
    match self {
      Self::Cli => "usr/local/bin/agent",
      Self::Desktop => "Applications/Lyonbrown.app",
    }
  }

  fn content_name(self) -> &'static str {
    match self {
      Self::Cli => "cli-content",
      Self::Desktop => "desktop-content",
    }
  }
}

pub(super) fn package(binaries: &Path, output: &Path, version: &str, cli_only: bool) -> Result<()> {
  // Only typed semantic versions and fixed product names enter distribution XML.
  let version = Version::parse(version)?;
  let stage = output.join("pkg-components");
  ensure!(
    !stage.exists(),
    "PKG stage already exists: {}",
    stage.display()
  );
  let daemon = stage.join("daemon/Library/Application Support/Lyonbrown");
  fs::create_dir_all(&daemon)?;
  copy_executable(&binaries.join("agentd"), &daemon.join("agentd"))?;
  let daemon = components::build(
    &stage,
    "daemon",
    "org.lyonbrown.agentd".to_owned(),
    &version,
    None,
  )?;
  for client in [Client::Cli, Client::Desktop] {
    if cli_only && matches!(client, Client::Desktop) {
      continue;
    }
    let (selector, content) = client_components(output, &stage, &version, client)?;
    distribution(
      output,
      &stage,
      &version,
      client,
      [&daemon, &selector, &content],
    )?;
  }
  Ok(())
}

fn client_components(
  output: &Path,
  stage: &Path,
  version: &Version,
  client: Client,
) -> Result<(Component, Component)> {
  let source = match client {
    Client::Cli => output
      .join(format!(
        "lyonbrown-cli-{version}-macos-{}",
        std::env::consts::ARCH
      ))
      .join(client.bundle()),
    Client::Desktop => output.join(client.bundle()),
  };
  let identity = bundle_identity(&source)?;
  let retained = Path::new("Library/Application Support/Lyonbrown/Clients")
    .join(client.name())
    .join(format!("{version}-{identity}"))
    .join(client.bundle());
  let installed = stage.join(client.content_name()).join(&retained);
  fs::create_dir_all(installed.parent().context("retained app has no parent")?)?;
  run(
    Command::new("ditto").arg(&source).arg(&installed),
    "stage immutable native client app",
  )?;
  ensure!(
    bundle_identity(&installed)? == identity,
    "copied native client app differs from selected bundle"
  );
  let content = components::build(
    stage,
    client.content_name(),
    format!("org.lyonbrown.{}.content.h{identity}", client.name()),
    version,
    Some(&retained),
  )?;
  let selector = stage.join(client.name()).join(client.selector());
  fs::create_dir_all(selector.parent().context("public selector has no parent")?)?;
  let mut target = Path::new("/").join(&retained);
  if matches!(client, Client::Cli) {
    target = target.join("Contents/MacOS/agent-cli");
  }
  symlink(target, selector)?;
  let selector = components::build(
    stage,
    client.name(),
    format!("org.lyonbrown.{}", client.name()),
    version,
    None,
  )?;
  Ok((selector, content))
}

fn distribution(
  output: &Path,
  stage: &Path,
  version: &Version,
  client: Client,
  components: [&Component; 3],
) -> Result<()> {
  let name = client.name();
  let mut choices = String::new();
  let mut references = String::new();
  for (index, component) in components.iter().enumerate() {
    choices.push_str(&format!(
      "<choice id=\"component{index}\" visible=\"false\"><pkg-ref id=\"{}\"/></choice>",
      component.identifier
    ));
    references.push_str(&format!(
      "<pkg-ref id=\"{}\" version=\"{version}\">{}</pkg-ref>",
      component.identifier, component.file
    ));
  }
  let xml = format!(
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<installer-gui-script minSpecVersion=\"2\"><title>Lyonbrown {name}</title><options customize=\"never\" require-scripts=\"false\"/><choices-outline><line choice=\"component0\"/><line choice=\"component1\"/><line choice=\"component2\"/></choices-outline>{choices}{references}</installer-gui-script>\n"
  );
  let distribution = stage.join(format!("{name}.xml"));
  fs::write(&distribution, xml)?;
  let installer = output.join(format!("Lyonbrown-{name}-{version}.pkg"));
  ensure!(
    !installer.exists(),
    "PKG already exists: {}",
    installer.display()
  );
  run(
    Command::new("productbuild")
      .arg("--distribution")
      .arg(distribution)
      .arg("--package-path")
      .arg(stage)
      .arg(installer),
    "build Apple product installer",
  )
}
