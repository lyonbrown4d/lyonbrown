use std::fs;

use agent_telemetry::{LogOptions, build_dispatch};
use anyhow::Result;

#[test]
fn guard_flushes_structured_events_into_private_file_and_retention_is_bounded() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let options = LogOptions {
    directory: temp.path().join("logs"),
    retained_files: 2,
    stderr: false,
  };
  fs::create_dir_all(&options.directory)?;
  for date in ["2000-01-01", "2000-01-02", "2000-01-03"] {
    fs::write(
      options.directory.join(format!("agentd.{date}.jsonl")),
      "old\n",
    )?;
  }
  let (dispatch, guard) = build_dispatch(&options, "agentd", "info")?;
  tracing::dispatcher::with_default(&dispatch, || {
    tracing::info!(
      run_id = "run-test",
      outcome = "accepted",
      "native daemon is ready"
    );
    tracing::debug!("filtered event");
  });
  assert_eq!(guard.dropped_lines(), 0);
  drop(guard);
  let entries = fs::read_dir(&options.directory)?.collect::<std::io::Result<Vec<_>>>()?;
  assert!(entries.len() <= options.retained_files);
  let content = entries
    .iter()
    .map(|entry| fs::read_to_string(entry.path()))
    .collect::<std::io::Result<Vec<_>>>()?
    .join("");
  assert!(content.contains("\"run_id\":\"run-test\""));
  assert!(content.contains("native daemon is ready"));
  assert!(!content.contains("filtered event"));
  assert!(!content.contains('\u{1b}'));
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
      fs::metadata(&options.directory)?.permissions().mode() & 0o777,
      0o700
    );
  }
  Ok(())
}

#[test]
fn zero_retention_and_unsafe_filename_are_rejected_before_writing() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let mut options = LogOptions::for_data_dir(temp.path());
  assert!(build_dispatch(&options, "../escape", "info").is_err());
  options.retained_files = 0;
  assert!(build_dispatch(&options, "agentd", "info").is_err());
  assert!(!options.directory.exists());
  Ok(())
}
