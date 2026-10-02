mod support;

#[cfg(unix)]
use agent_launch::DaemonSpawner;
use agent_launch::{CommandDaemonSpawner, LaunchRequest, SpawnRequest};
use std::ffi::OsStr;
use std::time::Duration;
use support::workspace;

#[test]
fn daemon_command_uses_direct_arguments_without_authentication_secret() {
  let request = LaunchRequest::new(
    workspace(),
    "runtime-config-v1",
    true,
    Duration::from_secs(1),
  )
  .unwrap();
  let spawn_request = SpawnRequest::from_launch_request(&request);
  let spawner = CommandDaemonSpawner::new("agentd", ["--foreground"]);

  let command = spawner.command_for(&spawn_request);
  let arguments: Vec<_> = command.get_args().collect();

  assert_eq!(command.get_program(), OsStr::new("agentd"));
  assert_eq!(arguments[0], OsStr::new("--foreground"));
  assert!(arguments.contains(&OsStr::new("--workspace")));
  assert!(arguments.contains(&workspace().as_os_str()));
  assert!(arguments.contains(&OsStr::new("--runtime-config-fingerprint")));
  assert!(arguments.contains(&OsStr::new("runtime-config-v1")));
  assert_eq!(
    command
      .get_envs()
      .find(|(key, _)| *key == "AGENT_AUTH_TOKEN"),
    Some((OsStr::new("AGENT_AUTH_TOKEN"), None))
  );
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_process_is_reaped_after_it_exits() {
  let temporary = tempfile::tempdir().expect("temporary root");
  let workspace = temporary.path().join("workspace");
  std::fs::create_dir(&workspace).expect("workspace");
  let workspace = workspace.canonicalize().expect("canonical workspace");
  let pid_file = temporary.path().join("child.pid");
  let request = LaunchRequest::new(workspace, "runtime-config-v1", true, Duration::from_secs(1))
    .expect("launch request");
  let spawn_request = SpawnRequest::from_launch_request(&request);
  let spawner = CommandDaemonSpawner::new(
    "/bin/sh",
    [
      "-c".into(),
      "printf '%s' \"$$\" > \"$1\"".into(),
      "agentd-test".into(),
      pid_file.as_os_str().to_owned(),
    ],
  );

  let monitor = spawner.spawn(&spawn_request).await.expect("spawn child");
  drop(monitor);
  let pid = wait_for_pid(&pid_file).await;

  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let output = tokio::process::Command::new("ps")
        .args(["-o", "state=", "-p", &pid])
        .output()
        .await
        .expect("inspect child process");
      if output.stdout.is_empty() {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("exited child must not remain a zombie");
}

#[cfg(unix)]
#[tokio::test]
async fn daemon_monitor_drains_large_stderr_and_reports_exit() {
  let temporary = tempfile::tempdir().expect("temporary root");
  let workspace = temporary
    .path()
    .canonicalize()
    .expect("canonical workspace");
  let request = LaunchRequest::new(workspace, "runtime-config-v1", true, Duration::from_secs(1))
    .expect("launch request");
  let spawn_request = SpawnRequest::from_launch_request(&request);
  let spawner = CommandDaemonSpawner::new(
    "/bin/sh",
    [
      "-c",
      "i=0; while [ \"$i\" -lt 10000 ]; do printf 'diagnostic-line\\n' >&2; i=$((i+1)); done; exit 23",
    ],
  );

  let mut monitor = spawner.spawn(&spawn_request).await.expect("spawn child");
  let exit = tokio::time::timeout(Duration::from_secs(2), monitor.wait())
    .await
    .expect("stderr drain must not block child exit");

  assert_eq!(exit.exit_code(), Some(23));
  assert!(!exit.stderr().is_empty());
  assert!(exit.stderr().len() <= 4_096);
}

#[cfg(unix)]
async fn wait_for_pid(path: &std::path::Path) -> String {
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if let Ok(pid) = tokio::fs::read_to_string(path).await {
        return pid;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("child must publish its pid")
}
