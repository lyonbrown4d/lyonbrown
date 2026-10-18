#![cfg(target_os = "macos")]

#[path = "../../../packaging/src/pkg.rs"]
mod pkg;
mod pkg_support;

use agent_launch::bundle_identity::bundle_identity;
use anyhow::{Context, Result, ensure};
use pkg_support::{DAEMON, VERSION, check_nonrelocation, fixture, leaves, source, xml};
use plist::Value;
use std::{
  fs,
  os::unix::fs::PermissionsExt,
  path::{Path, PathBuf},
  process::Command,
};

fn run(command: &mut Command, action: &str) -> Result<()> {
  let output = command.output().with_context(|| action.to_owned())?;
  ensure!(
    output.status.success(),
    "{action}: {}",
    String::from_utf8_lossy(&output.stderr)
  );
  Ok(())
}

fn copy_executable(source: &Path, destination: &Path) -> Result<()> {
  ensure!(fs::metadata(source)?.permissions().mode() & 0o111 != 0);
  fs::copy(source, destination)?;
  Ok(())
}

fn inspect(output: &Path, client: &str) -> Result<(String, PathBuf)> {
  let expanded = output.join(format!("expanded-{client}"));
  run(
    Command::new("pkgutil")
      .arg("--expand-full")
      .arg(output.join(format!("Lyonbrown-{client}-{VERSION}.pkg")))
      .arg(&expanded),
    "expand product without installing",
  )?;
  let identity = bundle_identity(&source(output, client))?;
  let identifier = format!("org.lyonbrown.{client}.content.h{identity}");
  let bundle_name = if client == "cli" {
    "Lyonbrown CLI.app"
  } else {
    "Lyonbrown.app"
  };
  let retained = Path::new("Library/Application Support/Lyonbrown/Clients")
    .join(client)
    .join(format!("{VERSION}-{identity}"))
    .join(bundle_name);
  let content = expanded.join(format!("{client}-content.pkg"));
  assert_eq!(
    xml(
      &content.join("PackageInfo"),
      "string(/pkg-info/@identifier)"
    )?,
    identifier
  );
  assert_eq!(
    xml(
      &content.join("PackageInfo"),
      "string(/pkg-info/@relocatable)"
    )?,
    "false"
  );
  assert_eq!(
    xml(&content.join("PackageInfo"), "count(/pkg-info/relocate/*)")?,
    "0"
  );
  assert_eq!(
    bundle_identity(&content.join("Payload").join(&retained))?,
    identity
  );
  assert_eq!(
    fs::read(
      content
        .join("Payload")
        .join(&retained)
        .join("Contents/Info.plist")
    )?,
    fs::read(source(output, client).join("Contents/Info.plist"))?
  );
  let bom = Command::new("lsbom")
    .arg("-s")
    .arg(content.join("Bom"))
    .output()?;
  ensure!(bom.status.success());
  let bom = String::from_utf8(bom.stdout)?;
  assert!(bom.contains(&format!(
    "./{}/Contents/MacOS/agent-{client}",
    retained.display()
  )));
  let analyzed =
    Value::from_file(output.join(format!("pkg-components/{client}-content-components.plist")))?;
  assert_eq!(
    check_nonrelocation(analyzed.as_array().expect("component array")),
    7
  );
  let main = &analyzed.as_array().expect("array")[0];
  assert_eq!(
    main.as_dictionary().expect("dictionary")["BundleOverwriteAction"].as_string(),
    Some("upgrade")
  );
  assert_eq!(
    main.as_dictionary().expect("dictionary")["BundleHasStrictIdentifier"].as_boolean(),
    Some(true)
  );
  assert_eq!(
    main.as_dictionary().expect("dictionary")["BundleIsVersionChecked"].as_boolean(),
    Some(true)
  );
  let selector = expanded.join(format!("{client}.pkg/Payload"));
  let link = if client == "cli" {
    "usr/local/bin/agent"
  } else {
    "Applications/Lyonbrown.app"
  };
  assert_eq!(leaves(&selector)?, vec![PathBuf::from(link)]);
  let target = if client == "cli" {
    retained.join("Contents/MacOS/agent-cli")
  } else {
    retained.clone()
  };
  assert_eq!(
    fs::read_link(selector.join(link))?,
    Path::new("/").join(target)
  );
  for name in [client, "daemon"] {
    let info = expanded.join(format!("{name}.pkg/PackageInfo"));
    assert_eq!(
      xml(&info, "string(/pkg-info/@identifier)")?,
      if name == "daemon" {
        "org.lyonbrown.agentd".to_owned()
      } else {
        format!("org.lyonbrown.{client}")
      }
    );
    assert_eq!(xml(&info, "count(/pkg-info/bundle)")?, "0");
    let analyzed =
      Value::from_file(output.join(format!("pkg-components/{name}-components.plist")))?;
    assert!(analyzed.as_array().expect("non-bundle array").is_empty());
  }
  let daemon = expanded.join("daemon.pkg/Payload");
  assert_eq!(
    leaves(&daemon)?,
    vec![PathBuf::from(
      "Library/Application Support/Lyonbrown/agentd"
    )]
  );
  assert_eq!(
    fs::read(daemon.join("Library/Application Support/Lyonbrown/agentd"))?,
    DAEMON
  );
  let distribution = expanded.join("Distribution");
  assert_eq!(
    xml(
      &distribution,
      "count(/installer-gui-script/pkg-ref[starts-with(normalize-space(text()), '#')])"
    )?,
    "3"
  );
  assert_eq!(
    xml(
      &distribution,
      &format!(
        "count(/installer-gui-script/pkg-ref[@id='{identifier}'][starts-with(normalize-space(text()), '#')])"
      )
    )?,
    "1"
  );
  assert_eq!(
    xml(
      &distribution,
      "count(/installer-gui-script/pkg-ref[@id='org.lyonbrown.agentd'][starts-with(normalize-space(text()), '#')])"
    )?,
    "1"
  );
  assert!(
    !expanded
      .join(if client == "cli" {
        "desktop.pkg"
      } else {
        "cli.pkg"
      })
      .exists()
  );
  Ok((identifier, retained))
}

#[test]
fn native_products_separate_content_receipts_selectors_and_shared_daemon() -> Result<()> {
  let temp = tempfile::Builder::new()
    .prefix("lyonbrown-pkg-native-")
    .tempdir_in("/private/tmp")?
    .keep();
  eprintln!("Non-installing native PKG evidence: {}", temp.display());
  let (binaries, first) = fixture(&temp, "first", false)?;
  pkg::package(&binaries, &first, VERSION, false)?;
  let first_cli = inspect(&first, "cli")?;
  let first_desktop = inspect(&first, "desktop")?;
  let (binaries, changed) = fixture(&temp, "changed", false)?;
  fs::write(
    source(&changed, "cli")
      .join("Contents/Frameworks/CEF.framework/Versions/A/Resources/locale.pak"),
    b"changed resource",
  )?;
  fs::write(source(&changed, "desktop").join("Contents/Frameworks/agent-desktop Helper (Renderer).app/Contents/MacOS/agent-desktop Helper (Renderer)"), b"changed helper")?;
  for client in ["cli", "desktop"] {
    assert_eq!(
      fs::read(source(&first, client).join(format!("Contents/MacOS/agent-{client}")))?,
      fs::read(source(&changed, client).join(format!("Contents/MacOS/agent-{client}")))?
    );
  }
  pkg::package(&binaries, &changed, VERSION, false)?;
  let changed_cli = inspect(&changed, "cli")?;
  let changed_desktop = inspect(&changed, "desktop")?;
  assert_ne!(
    first_cli.0, changed_cli.0,
    "resource-only change needs a new receipt"
  );
  assert_ne!(
    first_cli.1, changed_cli.1,
    "resource-only change needs a new path"
  );
  assert_ne!(
    first_desktop.0, changed_desktop.0,
    "helper-only change needs a new receipt"
  );
  assert_ne!(
    first_desktop.1, changed_desktop.1,
    "helper-only change needs a new path"
  );
  let (binaries, cli_only) = fixture(&temp, "cli-only", true)?;
  pkg::package(&binaries, &cli_only, VERSION, true)?;
  assert_eq!(
    first_cli,
    inspect(&cli_only, "cli")?,
    "same content must select the same retained path and receipt"
  );
  assert!(!cli_only.join("Lyonbrown-desktop-1.0.0.pkg").exists());
  assert!(!cli_only.join("pkg-components/desktop").exists());
  assert!(!cli_only.join("pkg-components/desktop-content.pkg").exists());
  let (binaries, invalid) = fixture(&temp, "invalid-version", true)?;
  assert!(pkg::package(&binaries, &invalid, "1.0.0\"/><bad/>", true).is_err());
  assert!(!invalid.join("pkg-components").exists());
  Ok(())
}
