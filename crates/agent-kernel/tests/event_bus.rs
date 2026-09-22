use agent_core::{AgentEvent, SessionId};
use agent_kernel::EventBus;
use chrono::Utc;

#[test]
fn zero_capacity_is_normalized_to_a_usable_channel() {
    let mut bus = EventBus::new(0);
    let mut receiver = bus.subscribe();
    bus.emit(AgentEvent::SessionStarted {
        session_id: SessionId::new(),
        ts: Utc::now(),
    });

    assert_eq!(receiver.try_recv().unwrap().kind(), "SessionStarted");
}
