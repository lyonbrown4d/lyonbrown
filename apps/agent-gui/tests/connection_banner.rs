use agent_gui::{state::ConnectionState, view::connection_banner_text};

#[test]
fn unavailable_banner_keeps_the_actionable_transport_error_visible() {
  let text = connection_banner_text(
    ConnectionState::Unavailable,
    false,
    Some("agentd did not become ready"),
  )
  .expect("unavailable connections need a banner");

  assert!(text.contains("UNAVAILABLE"));
  assert!(text.contains("agentd did not become ready"));
  assert!(text.contains("Retry"));
}

#[test]
fn reconnecting_banner_marks_cached_content_as_stale() {
  let text = connection_banner_text(ConnectionState::Reconnecting, true, Some("connection lost"))
    .expect("reconnecting connections need a banner");

  assert!(text.contains("RECONNECTING"));
  assert!(text.contains("stale"));
  assert!(text.contains("connection lost"));
}

#[test]
fn fresh_connected_state_does_not_render_a_warning_banner() {
  assert_eq!(
    connection_banner_text(ConnectionState::Connected, false, None),
    None
  );
}
