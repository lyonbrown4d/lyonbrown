#![cfg(unix)]

use std::{ffi::OsString, os::unix::ffi::OsStringExt, process::Command};

#[test]
fn invalid_environment_bytes_fail_without_panic_or_dotenv_fallback() {
  let directory = tempfile::tempdir().expect("tempdir");
  std::fs::write(
    directory.path().join(".env"),
    "LYONBROWN_GATEWAY_ADMIN_USERNAME=dotenv-admin\nLYONBROWN_GATEWAY_ADMIN_PASSWORD=dotenv-private-password\n",
  )
  .expect("dotenv credentials");
  let invalid = OsString::from_vec(b"private-environment-marker-\xff".to_vec());
  for (key, value) in [
    (
      OsString::from("LYONBROWN_GATEWAY_ADMIN_PASSWORD"),
      invalid.clone(),
    ),
    (invalid, OsString::from("private-environment-value")),
  ] {
    let output = Command::new(env!("CARGO_BIN_EXE_ai-gateway"))
      .env_clear()
      .env(key, value)
      .arg("--config")
      .arg(directory.path().join("gateway.toml"))
      .arg("--data-dir")
      .arg(directory.path())
      .output()
      .expect("gateway subprocess");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("gateway environment must be valid UTF-8"));
    for forbidden in ["panicked", "private-environment", "dotenv-private-password"] {
      assert!(!stderr.contains(forbidden));
    }
    assert!(!output.stderr.contains(&0xff));
  }
}
