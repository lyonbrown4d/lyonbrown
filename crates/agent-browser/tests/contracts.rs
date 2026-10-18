use agent_browser::{
  Action, BrowserError, Command, Frame, MAX_FRAME_BYTES, ProfileLease, validate_action,
  validate_url,
  wire::{self, Request},
};
use std::io::Cursor;

#[test]
fn browser_children_receive_platform_variables_without_daemon_or_provider_credentials() {
  let input = [
    "PATH",
    "HOME",
    "CEF_PATH",
    "DISPLAY",
    "LYONBROWN_TOKEN",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "AWS_SECRET_ACCESS_KEY",
    "CUSTOM_SECRET",
  ]
  .map(|key| (key.into(), "fixture".into()));
  let output = agent_browser::restricted_environment(input);
  let keys: Vec<_> = output
    .iter()
    .map(|(key, _)| key.to_str().expect("test key"))
    .collect();
  assert_eq!(keys, ["PATH", "HOME", "CEF_PATH", "DISPLAY"]);
}

#[test]
fn pipe_rejects_oversized_packet_before_reading_body() {
  let mut input = Cursor::new(((wire::MAX_PACKET_BYTES + 1) as u32).to_le_bytes());
  assert!(matches!(
    wire::read_packet::<Request>(&mut input),
    Err(BrowserError::InvalidInput(_))
  ));
  let request = Request {
    id: 42,
    command: Command::Observe {
      tab_id: "tab-a".into(),
      screenshot: true,
    },
  };
  let mut output = Vec::new();
  wire::write_packet(&mut output, &request).expect("encode bounded command");
  let decoded: Request =
    wire::read_packet(&mut Cursor::new(output)).expect("decode bounded command");
  assert_eq!(decoded.id, 42);
  assert!(matches!(
    decoded.command,
    Command::Observe {
      screenshot: true,
      ..
    }
  ));
}

#[test]
fn owned_bgra_frames_require_consistent_bounded_geometry() {
  let mut frame = Frame {
    tab_id: "tab-a".into(),
    revision: 2,
    sequence: 3,
    width: 2,
    height: 2,
    stride: 8,
    bgra: vec![0; 16],
  };
  assert!(frame.validate().is_ok());
  frame.stride = 4;
  assert!(frame.validate().is_err());
  frame.width = u32::MAX;
  assert!(frame.validate().is_err());
  frame.width = 2048;
  frame.height = 2049;
  frame.stride = frame.width * 4;
  assert!(frame.stride as usize * frame.height as usize > MAX_FRAME_BYTES);
  assert!(frame.validate().is_err());
}

#[test]
fn privileged_schemes_and_unbounded_actions_are_rejected() {
  for url in [
    "file:///etc/passwd",
    "javascript:alert(1)",
    "data:text/html,x",
    "https://user:secret@example.com",
    "about:settings",
  ] {
    assert!(validate_url(url).is_err());
  }
  assert!(validate_url("https://example.com").is_ok());
  assert!(validate_url("http://127.0.0.1:8080").is_ok());
  let mut action = Action {
    id: "action-a".into(),
    tab_id: "tab-a".into(),
    snapshot_id: "snapshot-a".into(),
    revision: 2,
    kind: "click".into(),
    element_ref: "e1".into(),
    text: String::new(),
    timeout_ms: 1000,
  };
  assert!(validate_action(&action).is_ok());
  action.timeout_ms = 30_001;
  assert!(validate_action(&action).is_err());
  action.timeout_ms = 1000;
  action.kind = "evaluate_javascript".into();
  assert!(validate_action(&action).is_err());
}

#[test]
fn profile_leases_exclude_other_spaces_and_release_on_drop() {
  let root = tempfile::tempdir().expect("profile test root");
  assert!(ProfileLease::acquire(root.path(), "../escape").is_err());
  let first = ProfileLease::acquire(root.path(), "profile-a").expect("first profile owner");
  assert!(matches!(
    ProfileLease::acquire(root.path(), "profile-a"),
    Err(BrowserError::Unavailable(_))
  ));
  let second = ProfileLease::acquire(root.path(), "profile-b").expect("isolated second profile");
  assert_ne!(first.directory(), second.directory());
  drop(first);
  assert!(ProfileLease::acquire(root.path(), "profile-a").is_ok());
}
