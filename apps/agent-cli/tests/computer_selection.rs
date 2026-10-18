#[path = "support/computer.rs"]
mod computer;

use agent_cli::computer::{Attach, ComputerError};
use agent_computer_runtime::DesktopScope;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn standalone_cli_never_adopts_the_process_of_a_reused_window_id() {
  let factory = computer::Factory::default();
  let mut selection = Attach {
    session: uuid::Uuid::now_v7().to_string(),
    window: Some(7),
    process_id: Some(19),
    display: None,
  };
  let target = selection.resolve_target(&factory).await.unwrap();
  assert_eq!(target.scope, DesktopScope::Window(7));
  assert_eq!(target.process_id, Some(19));

  factory.generation.fetch_add(1, Ordering::SeqCst);
  assert_eq!(
    selection.resolve_target(&factory).await.unwrap_err(),
    ComputerError::TargetMissing
  );
  assert!(factory.selected.lock().unwrap().is_empty());
  assert!(factory.attachments.lock().unwrap().is_empty());
  assert_eq!(factory.effects.captures.load(Ordering::SeqCst), 0);
  assert_eq!(factory.effects.inputs.load(Ordering::SeqCst), 0);

  selection.process_id = Some(20);
  assert_eq!(
    selection.resolve_target(&factory).await.unwrap().process_id,
    Some(20)
  );
  selection.process_id = Some(0);
  assert_eq!(
    selection.resolve_target(&factory).await.unwrap_err(),
    ComputerError::InvalidCommand
  );
  selection.window = None;
  selection.display = Some(2);
  selection.process_id = None;
  assert_eq!(
    selection.resolve_target(&factory).await.unwrap().scope,
    DesktopScope::Display(2)
  );
}
