//! Apple component analysis keeps every nested bundle at its immutable path.
use std::{path::Path, process::Command};

use anyhow::{Context, Result, ensure};
use plist::Value;
use semver::Version;

use crate::run;

pub(super) struct Component {
  pub(super) identifier: String,
  pub(super) file: String,
}

pub(super) fn build(
  stage: &Path,
  name: &str,
  identifier: String,
  version: &Version,
  bundle: Option<&Path>,
) -> Result<Component> {
  let root = stage.join(name);
  let component_plist = stage.join(format!("{name}-components.plist"));
  run(
    Command::new("pkgbuild")
      .arg("--analyze")
      .arg("--root")
      .arg(&root)
      .arg(&component_plist),
    "analyze Apple PKG bundles",
  )?;
  let mut analyzed = Value::from_file(&component_plist)?;
  nonrelocatable(&mut analyzed, &root, bundle)?;
  analyzed.to_file_xml(&component_plist)?;
  let file = format!("{name}.pkg");
  run(
    Command::new("pkgbuild")
      .arg("--root")
      .arg(&root)
      .arg("--component-plist")
      .arg(&component_plist)
      .args([
        "--identifier",
        &identifier,
        "--version",
        &version.to_string(),
      ])
      .args(["--install-location", "/"])
      .arg(stage.join(&file)),
    "build Apple PKG component",
  )?;
  Ok(Component { identifier, file })
}

pub(crate) fn nonrelocatable(
  analyzed: &mut Value,
  root: &Path,
  bundle: Option<&Path>,
) -> Result<()> {
  let records = analyzed
    .as_array_mut()
    .context("Apple component analysis must be an array")?;
  let Some(bundle) = bundle else {
    ensure!(
      records.is_empty(),
      "non-bundle component contains bundle records"
    );
    return Ok(());
  };
  ensure!(
    !records.is_empty(),
    "native app was absent from component analysis"
  );
  let mut found = false;
  pin_records(records, root, bundle, &mut found)?;
  ensure!(found, "native app was absent from component analysis");
  Ok(())
}

fn pin_records(records: &mut [Value], root: &Path, bundle: &Path, found: &mut bool) -> Result<()> {
  for record in records {
    let record = record
      .as_dictionary_mut()
      .context("Apple bundle analysis record must be a dictionary")?;
    let path = Path::new(
      record
        .get("RootRelativeBundlePath")
        .and_then(Value::as_string)
        .context("Apple bundle analysis requires a bundle path")?,
    );
    ensure!(
      path
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
        && path.starts_with(bundle)
        && root.join(path).is_dir(),
      "Apple bundle analysis path is outside the staged app"
    );
    *found |= path == bundle;
    for key in [
      "BundleIsRelocatable",
      "BundleHasStrictIdentifier",
      "BundleIsVersionChecked",
    ] {
      ensure!(
        record
          .get(key)
          .is_none_or(|value| value.as_boolean().is_some()),
        "Apple bundle analysis {key} must be boolean"
      );
    }
    if let Some(action) = record.get("BundleOverwriteAction") {
      ensure!(
        matches!(action.as_string(), Some("" | "upgrade" | "update")),
        "Apple bundle analysis has an invalid overwrite action"
      );
    }
    record.insert("BundleIsRelocatable".to_owned(), Value::Boolean(false));
    if let Some(children) = record.get_mut("ChildBundles") {
      pin_records(
        children
          .as_array_mut()
          .context("Apple child bundle analysis must be an array")?,
        root,
        bundle,
        found,
      )?;
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::nonrelocatable;
  use anyhow::Result;
  use plist::{Dictionary, Value};
  use std::{fs, path::Path};

  #[test]
  fn malformed_analysis_fails_and_nested_flags_preserve_bundle_policy() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = Path::new("Client.app");
    let child = path.join("Contents/Frameworks/Helper.app");
    fs::create_dir_all(temp.path().join(&child))?;
    let record = |path: &Path| {
      Value::Dictionary(Dictionary::from_iter([
        (
          "RootRelativeBundlePath",
          Value::String(path.to_string_lossy().into_owned()),
        ),
        ("BundleOverwriteAction", Value::String("upgrade".into())),
        ("BundleIsRelocatable", Value::Boolean(true)),
        ("BundleIsVersionChecked", Value::Boolean(true)),
        ("BundleHasStrictIdentifier", Value::Boolean(true)),
      ]))
    };
    let mut top = record(path);
    top
      .as_dictionary_mut()
      .expect("record")
      .insert("ChildBundles".into(), Value::Array(vec![record(&child)]));
    let input = Value::Array(vec![top]);
    let mut valid = input.clone();
    nonrelocatable(&mut valid, temp.path(), Some(path))?;
    let top = valid.as_array().expect("array")[0]
      .as_dictionary()
      .expect("record");
    assert_eq!(top["BundleIsRelocatable"].as_boolean(), Some(false));
    assert_eq!(top["BundleIsVersionChecked"].as_boolean(), Some(true));
    assert_eq!(top["BundleHasStrictIdentifier"].as_boolean(), Some(true));
    assert_eq!(top["BundleOverwriteAction"].as_string(), Some("upgrade"));
    let children = top["ChildBundles"].as_array().expect("children");
    assert_eq!(
      children[0].as_dictionary().expect("child")["BundleIsRelocatable"].as_boolean(),
      Some(false)
    );
    for (key, value) in [
      (
        "RootRelativeBundlePath",
        Value::String("../Client.app".into()),
      ),
      ("BundleIsVersionChecked", Value::String("true".into())),
      ("BundleOverwriteAction", Value::String("invalid".into())),
      ("ChildBundles", Value::String("invalid".into())),
    ] {
      let mut malformed = input.clone();
      malformed.as_array_mut().expect("array")[0]
        .as_dictionary_mut()
        .expect("record")
        .insert(key.into(), value);
      assert!(
        nonrelocatable(&mut malformed, temp.path(), Some(path)).is_err(),
        "must reject {key}"
      );
    }
    for mut malformed in [
      Value::String("invalid".into()),
      Value::Array(vec![Value::Boolean(false)]),
      Value::Array(vec![]),
    ] {
      assert!(nonrelocatable(&mut malformed, temp.path(), Some(path)).is_err());
    }
    let mut nonbundle = input;
    assert!(nonrelocatable(&mut nonbundle, temp.path(), None).is_err());
    nonrelocatable(&mut Value::Array(vec![]), temp.path(), None)?;
    Ok(())
  }
}
