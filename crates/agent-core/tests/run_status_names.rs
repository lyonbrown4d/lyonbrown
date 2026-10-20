use agent_core::RunStatus;

#[test]
fn run_status_names_preserve_persisted_strings() {
  for (status, expected) in [
    (RunStatus::Accepted, "accepted"),
    (RunStatus::Running, "running"),
    (RunStatus::AwaitingApproval, "awaiting_approval"),
    (RunStatus::CancelRequested, "cancel_requested"),
    (RunStatus::Completed, "completed"),
    (RunStatus::Failed, "failed"),
    (RunStatus::Cancelled, "cancelled"),
    (RunStatus::Uncertain, "uncertain"),
  ] {
    let name: &'static str = status.as_str();
    assert_eq!(name, expected);
  }
}
