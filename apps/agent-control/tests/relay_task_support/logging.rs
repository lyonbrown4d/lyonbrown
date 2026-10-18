use std::sync::OnceLock;

/// Keep ordinary process diagnostics available for local relay acceptance failures.
pub fn init() {
  static LOGS: OnceLock<agent_telemetry::LogGuard> = OnceLock::new();
  LOGS.get_or_init(|| {
    let directory = tempfile::tempdir().unwrap().keep();
    eprintln!("Preserved relay acceptance logs: {}", directory.display());
    agent_telemetry::init_with_options(
      &agent_telemetry::LogOptions {
        directory,
        retained_files: 2,
        stderr: true,
      },
      "relay-acceptance",
      "warn,agent_distributed=debug,agent_control=debug",
    )
    .unwrap()
  });
}
