use agent_core::AgentEvent;
use tokio::sync::broadcast;

use crate::tui::TuiState;

pub fn final_drain(receiver: &mut Option<broadcast::Receiver<AgentEvent>>, state: &mut TuiState) {
  drain_events(receiver, state);
}

pub(super) fn drain_events(
  receiver: &mut Option<broadcast::Receiver<AgentEvent>>,
  state: &mut TuiState,
) {
  let Some(receiver) = receiver else { return };
  loop {
    match receiver.try_recv() {
      Ok(event) => state.reduce(event),
      Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed) => {
        break;
      }
      Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
        state.set_error(format!("event stream lagged by {skipped} events"));
      }
    }
  }
}
