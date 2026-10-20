use anyhow::{Context, Result};
use serde_json::json;
use std::path::PathBuf;

fn artifact(name: &str, kind: &str, executable: Option<&str>) -> String {
  json!({
    "reason": "compiler-artifact",
    "package_id": "path+file:///fixture#0.1.0",
    "target": { "name": name, "kind": [kind], "src_path": "/fixture/src/main.rs" },
    "profile": {
      "opt_level": "0", "debuginfo": 0, "debug_assertions": true,
      "overflow_checks": true, "test": false
    },
    "features": [], "filenames": [], "executable": executable, "fresh": false
  })
  .to_string()
}

#[tokio::test]
async fn discovery_selects_only_the_requested_binary_and_ignores_unrelated_output() -> Result<()> {
  for package in ["agentd", "ai-gateway"] {
    let selected = "/fixture/custom target/product with spaces";
    let input = [
      "tool chatter".into(),
      "{malformed".into(),
      artifact("unrelated", "bin", Some("/wrong-name")),
      artifact(package, "example", Some("/wrong-kind")),
      artifact(package, "bin", Some(selected)),
      artifact(package, "bin", None),
      json!({ "reason": "build-finished", "success": true }).to_string(),
    ]
    .join("\n");
    assert_eq!(
      crate::cargo::executable(input.as_bytes(), package).await?,
      PathBuf::from(selected)
    );
  }
  let missing = artifact("agentd", "lib", Some("/library"));
  assert!(
    crate::cargo::executable(missing.as_bytes(), "agentd")
      .await
      .is_err()
  );
  Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn build_drains_output_and_rejects_nonzero_exit_after_reporting_an_artifact() -> Result<()> {
  for exit in [0, 7] {
    let root = tempfile::tempdir()?;
    let messages = root.path().join("messages.jsonl");
    std::fs::write(
      &messages,
      artifact("agentd", "bin", Some("/fixture/agentd")),
    )?;
    let mut command = std::process::Command::new("/bin/sh");
    command.current_dir(root.path()).args([
      "-c",
      r#"cat messages.jsonl; printf '\n'; i=0; while [ $i -lt 8192 ]; do printf 'non-JSON build output after artifact\n'; i=$((i+1)); done; printf done > drained; exit "$1""#,
      "fixture",
      &exit.to_string(),
    ]).stdout(std::process::Stdio::piped());
    let result = tokio::time::timeout(
      std::time::Duration::from_secs(5),
      super::run_build(command, "agentd"),
    )
    .await?;
    assert!(
      root.path().join("drained").exists(),
      "runner must consume output through EOF"
    );
    if exit == 0 {
      assert_eq!(result?, PathBuf::from("/fixture/agentd"));
    } else {
      assert!(result.unwrap_err().to_string().contains("build failed"));
    }
  }
  Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn dropping_a_build_future_terminates_its_descendant_processes() -> Result<()> {
  use std::{process::Stdio, time::Duration};
  let root = tempfile::tempdir()?;
  let mut command = std::process::Command::new("/bin/sh");
  command
    .current_dir(root.path())
    .args(["-c", "sleep 60 & echo $! > descendant.pid; wait"])
    .stdout(Stdio::piped());
  let mut builds = tokio::task::JoinSet::new();
  builds.spawn(super::run_build(command, "agentd"));
  let marker = root.path().join("descendant.pid");
  let pid = tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      if let Ok(contents) = std::fs::read_to_string(&marker)
        && let Ok(pid) = contents.trim().parse::<u32>()
      {
        break pid.to_string();
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .context("build fixture did not publish its descendant PID")?;
  builds.abort_all();
  assert!(
    builds
      .join_next()
      .await
      .context("missing build task")?
      .is_err()
  );
  wait_for_termination(&pid).await
}

#[cfg(unix)]
#[tokio::test]
async fn output_collection_terminates_descendants_after_the_leader_exits() -> Result<()> {
  use std::{process::Stdio, time::Duration};
  let mut command = std::process::Command::new("/bin/sh");
  command
    .args(["-c", "sleep 60 & echo $!; exit 0"])
    .stdout(Stdio::piped());
  let child = crate::process::spawn_command(command)?;
  let output = tokio::time::timeout(
    Duration::from_secs(5),
    Box::into_pin(child.wait_with_output()),
  )
  .await
  .context("exited build leader left a descendant holding its output pipe")??;
  assert!(output.status.success());
  let pid = std::str::from_utf8(&output.stdout)?.trim().parse::<u32>()?;
  wait_for_termination(&pid.to_string()).await
}

#[cfg(unix)]
async fn wait_for_termination(pid: &str) -> Result<()> {
  use std::{process::Stdio, time::Duration};
  tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      let status = std::process::Command::new("kill")
        .args(["-0", pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("inspect cancelled build descendant")?;
      if !status.success() {
        break Ok::<(), anyhow::Error>(());
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .context("cancelled build descendant did not terminate")??;
  Ok(())
}
