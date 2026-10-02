use agent_app::{AgentClientError, AvailabilityKind, ProtocolErrorKind};

#[test]
fn availability_errors_have_a_stable_kind_and_human_message() {
  let error = AgentClientError::unavailable(
    AvailabilityKind::Timeout,
    "daemon did not respond before the deadline",
  );

  assert_eq!(AvailabilityKind::Timeout.code(), "timeout");
  assert_eq!(AvailabilityKind::Timeout.to_string(), "timeout");
  assert_eq!(
    error,
    AgentClientError::Unavailable {
      kind: AvailabilityKind::Timeout,
      message: "daemon did not respond before the deadline".into(),
    }
  );
  assert_eq!(
    error.to_string(),
    "agent client unavailable (timeout): daemon did not respond before the deadline"
  );
}

#[test]
fn protocol_errors_have_a_stable_kind_and_human_message() {
  let error = AgentClientError::protocol(
    ProtocolErrorKind::IncompatibleVersion,
    "server requires protocol v2",
  );

  assert_eq!(
    ProtocolErrorKind::IncompatibleVersion.code(),
    "incompatible_version"
  );
  assert_eq!(
    ProtocolErrorKind::IncompatibleVersion.to_string(),
    "incompatible_version"
  );
  assert_eq!(
    error,
    AgentClientError::Protocol {
      kind: ProtocolErrorKind::IncompatibleVersion,
      message: "server requires protocol v2".into(),
    }
  );
  assert_eq!(
    error.to_string(),
    "agent protocol failed (incompatible_version): server requires protocol v2"
  );
}

#[test]
fn authentication_failure_has_a_stable_protocol_code() {
  assert_eq!(
    ProtocolErrorKind::AuthenticationFailed.code(),
    "authentication_failed"
  );
  assert_eq!(
    ProtocolErrorKind::AuthenticationFailed.to_string(),
    "authentication_failed"
  );
}
