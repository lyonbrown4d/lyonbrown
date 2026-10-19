#![cfg(unix)]
use std::{ffi::OsString, os::unix::ffi::OsStringExt, process::Command};

#[test]
fn non_unicode_environment_fails_without_panicking_or_echoing_its_value() {
  let output = Command::new(env!("CARGO_BIN_EXE_model-mock"))
    .env(
      "LYONBROWN_MODEL_MOCK_LISTEN",
      OsString::from_vec(b"private-environment-value-\xff".to_vec()),
    )
    .output()
    .expect("launch mock process");
  assert!(!output.status.success());
  let error = String::from_utf8_lossy(&output.stderr);
  assert!(error.contains("mock environment must be valid UTF-8"));
  assert!(!error.contains("panicked"));
  assert!(!error.contains("private-environment-value"));
}
