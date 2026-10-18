#![cfg(unix)]

#[path = "../src/native/mac_paths.rs"]
mod paths;

use std::{
  fs,
  os::unix::fs::symlink,
  path::{Path, PathBuf},
};

struct Bundle {
  root: tempfile::TempDir,
  executable: PathBuf,
  helper: PathBuf,
  framework: PathBuf,
  sandbox: PathBuf,
}

fn file(path: &Path) {
  fs::create_dir_all(path.parent().expect("fixture parent")).expect("fixture directories");
  fs::write(path, []).expect("fixture file");
}

impl Bundle {
  fn new() -> Self {
    let root = tempfile::tempdir().expect("fixture root");
    let bundle = root.path().join("retained/Lyonbrown.app/Contents");
    let executable = bundle.join("MacOS/agent");
    let helper = bundle.join("Frameworks/agent Helper.app/Contents/MacOS/agent Helper");
    let framework =
      bundle.join("Frameworks/Chromium Embedded Framework.framework/Chromium Embedded Framework");
    let sandbox = bundle
      .join("Frameworks/Chromium Embedded Framework.framework/Libraries/libcef_sandbox.dylib");
    for path in [&executable, &helper, &framework, &sandbox] {
      file(path);
    }
    Self {
      root,
      executable,
      helper,
      framework,
      sandbox,
    }
  }

  fn public_link(&self, relative: bool) -> PathBuf {
    let link = self.root.path().join(if relative {
      "relative-agent"
    } else {
      "absolute-agent"
    });
    let target = if relative {
      self
        .executable
        .strip_prefix(self.root.path())
        .expect("relative fixture")
    } else {
      &self.executable
    };
    symlink(target, &link).expect("public executable symlink");
    link
  }
}

#[test]
fn public_symlinks_keep_original_bundle_helper_identity_and_framework() {
  let bundle = Bundle::new();
  let fallback = bundle.root.path().join("fallback");
  file(&fallback.join("Chromium Embedded Framework.framework/Chromium Embedded Framework"));
  for relative in [false, true] {
    let public = bundle.public_link(relative);
    assert_eq!(
      paths::helper_executable(&public).expect("packaged helper"),
      bundle.helper.canonicalize().expect("helper path")
    );
    assert_eq!(
      paths::framework_library(&public, Some(fallback.clone()), Some(fallback.clone()))
        .expect("packaged framework")
        .canonicalize()
        .expect("framework path"),
      bundle.framework.canonicalize().expect("original framework")
    );
  }
}

#[test]
fn helper_framework_lookup_resolves_alias_but_sandbox_requires_packaged_location() {
  let bundle = Bundle::new();
  let public = bundle.root.path().join("public-helper");
  symlink(&bundle.helper, &public).expect("helper symlink");
  assert_eq!(
    paths::framework_library(&public, None, None)
      .expect("helper framework")
      .canonicalize()
      .expect("framework path"),
    bundle.framework.canonicalize().expect("original framework")
  );
  assert_eq!(
    paths::sandbox_library(&bundle.helper)
      .expect("helper sandbox")
      .canonicalize()
      .expect("sandbox path"),
    bundle.sandbox.canonicalize().expect("original sandbox")
  );
  assert!(
    paths::sandbox_library(&public)
      .expect_err("external helper alias is unsupported by the maintained sandbox loader")
      .to_string()
      .contains("sandbox library is missing")
  );
  fs::remove_file(&bundle.sandbox).expect("remove sandbox");
  assert!(
    paths::sandbox_library(&bundle.helper)
      .expect_err("missing sandbox")
      .to_string()
      .contains("sandbox library is missing")
  );
  fs::remove_file(&bundle.framework).expect("remove framework");
  assert!(
    paths::framework_library(&public, None, None)
      .expect_err("missing framework")
      .to_string()
      .contains("CEF framework missing")
  );
}

#[test]
fn missing_and_broken_helpers_are_rejected() {
  let bundle = Bundle::new();
  let public = bundle.public_link(true);
  fs::remove_file(&bundle.helper).expect("remove helper");
  for broken in [false, true] {
    if broken {
      symlink("missing-helper", &bundle.helper).expect("broken helper symlink");
    }
    assert!(
      paths::helper_executable(&public)
        .expect_err("missing helper")
        .to_string()
        .contains("helper bundle is missing")
    );
  }
}

#[test]
fn unavailable_and_malformed_executables_fail_explicitly() {
  let root = tempfile::tempdir().expect("fixture root");
  let missing = root.path().join("missing");
  let broken = root.path().join("broken");
  symlink(&missing, &broken).expect("broken public link");
  for executable in [&missing, &broken] {
    assert!(
      paths::helper_executable(executable)
        .expect_err("unavailable executable")
        .to_string()
        .contains("cannot resolve native client executable")
    );
    assert!(paths::framework_library(executable, None, None).is_err());
    assert!(paths::sandbox_library(executable).is_err());
  }
  for relative in ["agent", "Wrong.app/Other/MacOS/agent"] {
    let executable = root.path().join(relative);
    file(&executable);
    assert!(
      paths::helper_executable(&executable)
        .expect_err("malformed app")
        .to_string()
        .contains("requires the native client app bundle")
    );
  }
}
