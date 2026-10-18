mod process_support;
use agent_workspace::process::*;
use process_support::*;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn approved_execution_preserves_exact_arguments_cwd_and_selected_environment() {
  assert!(
    std::env::var_os("PATH").is_some(),
    "fixture verifies an ambient variable is removed"
  );
  let root = tempfile::tempdir().unwrap();
  std::fs::create_dir(root.path().join("nested")).unwrap();
  let approval = Arc::new(Approval {
    allow: true,
    ..Approval::default()
  });
  let executor = CommandExecutor::open(root.path(), policy(), approval.clone()).unwrap();
  let mut request = request(vec![
    "report".into(),
    "空 白;$(echo injected)".into(),
    "--literal=\"quoted\"".into(),
  ]);
  request.cwd = "nested".into();
  let outcome = executor
    .execute(request.clone(), CancellationToken::new())
    .await;
  assert_eq!(outcome.status, CommandStatus::Succeeded);
  assert_eq!(outcome.exit_code, Some(0));
  assert!(
    outcome
      .stdout
      .contains("arg:空 白;$(echo injected)\narg:--literal=\"quoted\"\n")
  );
  assert!(outcome.stdout.contains(&format!(
    "cwd:{}\n",
    root.path().join("nested").canonicalize().unwrap().display()
  )));
  assert!(
    outcome
      .stdout
      .contains("selected:chosen\nambient:\nextra:\nstdin:0\n")
  );
  assert_eq!(outcome.stderr, "stderr:确认");
  let seen = approval.seen.lock().unwrap();
  assert_eq!(seen.len(), 1);
  assert_eq!(seen[0].0, request);
  assert_eq!(
    seen[0].1.cwd,
    root.path().join("nested").canonicalize().unwrap()
  );
}

#[tokio::test]
async fn both_pipes_drain_without_pipe_capacity_deadlock() {
  let root = tempfile::tempdir().unwrap();
  let mut request = request(vec!["dual".into()]);
  request.output_limit_bytes = 250_000;
  let outcome = executor(root.path())
    .execute(request, CancellationToken::new())
    .await;
  assert_eq!(outcome.status, CommandStatus::Succeeded);
  assert_eq!(outcome.stdout, "o".repeat(100_000));
  assert_eq!(outcome.stderr, "e".repeat(100_000));
}

#[tokio::test]
async fn deny_invalid_scope_and_prestart_cancel_never_spawn() {
  let root = tempfile::tempdir().unwrap();
  let marker = root.path().join("effect");
  let request = request(vec!["mark".into(), marker.display().to_string()]);
  let denied =
    CommandExecutor::open(root.path(), policy(), Arc::new(DenyCommandAuthorization)).unwrap();
  assert_eq!(
    denied
      .execute(request.clone(), CancellationToken::new())
      .await
      .status,
    CommandStatus::Denied
  );
  let executor = executor(root.path());
  for cwd in ["../", "/"] {
    let mut unsafe_request = request.clone();
    unsafe_request.cwd = cwd.into();
    assert_eq!(
      executor
        .execute(unsafe_request, CancellationToken::new())
        .await
        .status,
      CommandStatus::Rejected
    );
  }
  let mut unsafe_request = request.clone();
  unsafe_request.executable = "relative-program".into();
  assert_eq!(
    executor
      .execute(unsafe_request, CancellationToken::new())
      .await
      .status,
    CommandStatus::Rejected
  );
  let copied = root.path().join(if cfg!(windows) {
    "untrusted.exe"
  } else {
    "untrusted"
  });
  std::fs::copy(fixture(), &copied).unwrap();
  let mut unsafe_request = request.clone();
  unsafe_request.executable = copied;
  assert_eq!(
    executor
      .execute(unsafe_request, CancellationToken::new())
      .await
      .status,
    CommandStatus::Rejected
  );
  let cancel = CancellationToken::new();
  cancel.cancel();
  assert_eq!(
    executor.execute(request, cancel).await.status,
    CommandStatus::CancelledBeforeStart
  );
  assert!(!marker.exists());
}

#[tokio::test]
async fn cancellation_and_deadline_while_awaiting_approval_never_spawn() {
  for cancel_during_approval in [true, false] {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("effect");
    let approval = Arc::new(ApprovalGate::default());
    let executor = CommandExecutor::open(root.path(), policy(), approval.clone()).unwrap();
    let mut request = request(vec!["mark".into(), marker.display().to_string()]);
    request.timeout_ms = 100;
    let cancel = CancellationToken::new();
    let task = tokio::spawn({
      let cancel = cancel.clone();
      async move { executor.execute(request, cancel).await }
    });
    approval.entered.cancelled().await;
    if cancel_during_approval {
      cancel.cancel();
    }
    let outcome = task.await.unwrap();
    assert_eq!(
      outcome.status,
      if cancel_during_approval {
        CommandStatus::CancelledBeforeStart
      } else {
        CommandStatus::Rejected
      }
    );
    assert!(!marker.exists());
  }
}

#[cfg(unix)]
#[tokio::test]
async fn cwd_scope_changed_during_approval_is_rejected_before_spawn() {
  let root = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  std::fs::create_dir(root.path().join("inside")).unwrap();
  let link = root.path().join("link");
  std::os::unix::fs::symlink(root.path().join("inside"), &link).unwrap();
  let approval = Arc::new(ApprovalGate::default());
  let executor = CommandExecutor::open(root.path(), policy(), approval.clone()).unwrap();
  let mut request = request(vec!["mark".into(), "effect".into()]);
  request.cwd = "link".into();
  let task = tokio::spawn(async move { executor.execute(request, CancellationToken::new()).await });
  approval.entered.cancelled().await;
  std::fs::remove_file(&link).unwrap();
  std::os::unix::fs::symlink(outside.path(), &link).unwrap();
  approval.release.cancel();
  assert_eq!(task.await.unwrap().status, CommandStatus::Rejected);
  assert!(!outside.path().join("effect").exists());
  assert!(!root.path().join("inside/effect").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn cwd_symlink_escape_is_rejected_before_authorization() {
  let root = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
  let approval = Arc::new(Approval {
    allow: true,
    ..Approval::default()
  });
  let executor = CommandExecutor::open(root.path(), policy(), approval.clone()).unwrap();
  let mut request = request(vec!["mark".into(), "effect".into()]);
  request.cwd = "escape".into();
  assert_eq!(
    executor
      .execute(request, CancellationToken::new())
      .await
      .status,
    CommandStatus::Rejected
  );
  assert!(approval.seen.lock().unwrap().is_empty());
  assert!(!outside.path().join("effect").exists());
}

#[tokio::test]
async fn combined_output_budget_stops_execution_without_retry() {
  let root = tempfile::tempdir().unwrap();
  let marker = root.path().join("effect");
  let mut request = request(vec!["flood".into(), marker.display().to_string()]);
  request.output_limit_bytes = 1000;
  let outcome = executor(root.path())
    .execute(request, CancellationToken::new())
    .await;
  assert_eq!(outcome.status, CommandStatus::Uncertain);
  assert_eq!(outcome.reason, Some(CommandStopReason::OutputLimit));
  assert!(outcome.output_truncated);
  assert_eq!(outcome.stdout.len() + outcome.stderr.len(), 1000);
  assert!(!marker.exists());
}

#[tokio::test]
async fn nonzero_exit_reports_known_failure() {
  let root = tempfile::tempdir().unwrap();
  let outcome = run(root.path(), vec!["fail".into()]).await;
  assert_eq!(outcome.status, CommandStatus::Failed);
  assert_eq!(outcome.exit_code, Some(17));
  assert_eq!(outcome.stderr, "known failure");
}

#[tokio::test]
async fn cancellation_deadline_drop_and_leader_exit_clean_descendants() {
  for scenario in ["cancel", "deadline", "drop", "leader_exit"] {
    let root = tempfile::tempdir().unwrap();
    let ready = root.path().join("ready");
    let marker = root.path().join("effect");
    let mut request = request(vec![
      if scenario == "leader_exit" {
        "tree-exit"
      } else {
        "tree"
      }
      .into(),
      ready.display().to_string(),
      marker.display().to_string(),
    ]);
    if scenario == "deadline" {
      request.timeout_ms = 800;
    }
    let cancel = CancellationToken::new();
    let executor = executor(root.path());
    let task = tokio::spawn({
      let cancel = cancel.clone();
      async move { executor.execute(request, cancel).await }
    });
    wait_ready(&ready).await;
    if scenario == "cancel" {
      cancel.cancel();
    }
    if scenario == "drop" {
      task.abort();
      assert!(task.await.unwrap_err().is_cancelled());
    } else {
      let outcome = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
      match scenario {
        "cancel" => assert_eq!(outcome.reason, Some(CommandStopReason::Cancelled)),
        "deadline" => assert_eq!(outcome.reason, Some(CommandStopReason::Deadline)),
        _ => assert_eq!(outcome.status, CommandStatus::Succeeded),
      }
      if scenario != "leader_exit" {
        assert_eq!(outcome.status, CommandStatus::Uncertain);
      }
    }
    no_descendant_effect(&marker).await;
  }
}
