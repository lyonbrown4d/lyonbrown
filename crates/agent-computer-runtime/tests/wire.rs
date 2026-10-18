use agent_computer::*;
use agent_computer_runtime::{wire::*, *};
use std::io::{Cursor, Read};
use uuid::Uuid;

#[test]
fn private_command_roundtrips_exact_binding_and_rejects_trailing_bytes() {
  let request = Request {
    id: 42,
    command: Command {
      binding: RunBinding {
        daemon_session_id: Uuid::new_v4().to_string(),
        run_id: Uuid::new_v4().to_string(),
        owner_epoch: 7,
      },
      operation: Operation::AcquireLease { ttl_ms: 1000 },
    },
  };
  let mut packet = Vec::new();
  write_packet(&mut packet, &request).expect("write");
  let decoded: Request = read_packet(&mut Cursor::new(packet)).expect("read");
  assert_eq!(decoded.id, 42);
  assert_eq!(decoded.command.binding, request.command.binding);
  assert!(matches!(
    decoded.command.operation,
    Operation::AcquireLease { ttl_ms: 1000 }
  ));
  let mut bytes = encode(&request).expect("encode");
  bytes.push(0);
  assert!(matches!(
    decode::<Request>(&bytes),
    Err(RuntimeError::Protocol)
  ));
}

#[test]
fn oversized_length_is_rejected_before_payload_read_or_allocation() {
  struct HeaderOnly {
    header: Cursor<[u8; 4]>,
    payload_reads: usize,
  }
  impl Read for HeaderOnly {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
      if self.header.position() >= 4 {
        self.payload_reads += 1;
        panic!("oversized payload read");
      }
      self.header.read(bytes)
    }
  }
  let mut reader = HeaderOnly {
    header: Cursor::new(((MAX_PACKET_BYTES + 1) as u32).to_le_bytes()),
    payload_reads: 0,
  };
  assert!(matches!(
    read_packet::<Request>(&mut reader),
    Err(RuntimeError::PacketTooLarge)
  ));
  assert_eq!(reader.payload_reads, 0);
}

#[test]
fn full_explicit_rgba_ceiling_is_transported_without_downsampling_or_truncation() {
  let session = DesktopSession {
    id: Uuid::new_v4(),
    identity: DesktopIdentity {
      node_id: "test".into(),
      user_session: "test".into(),
    },
    scope: TargetScope::Display(1),
  };
  let response = Response {
    id: 1,
    result: Ok(Reply::Snapshot(DesktopSnapshot {
      id: Uuid::new_v4(),
      revision: 1,
      lease_id: Uuid::new_v4(),
      session,
      observation: DesktopObservation {
        bounds: Bounds {
          x: 0,
          y: 0,
          width: 4096,
          height: 4096,
        },
        screenshot: Screenshot {
          width: 4096,
          height: 4096,
          rgba: vec![123; 64 * 1024 * 1024],
        },
        windows: vec![],
        windows_truncated: false,
        accessibility: AccessibilityObservation {
          state: PermissionState::Unsupported,
          nodes: vec![],
          truncated: false,
        },
      },
    })),
  };
  let bytes = encode(&response).expect("explicit 64 MiB image fits");
  assert!(bytes.len() <= MAX_PACKET_BYTES);
  let decoded: Response = decode(&bytes).expect("decode");
  let snapshot = match decoded.result.expect("response") {
    Reply::Snapshot(snapshot) => snapshot,
    _ => panic!("snapshot"),
  };
  assert_eq!(snapshot.observation.screenshot.width, 4096);
  assert_eq!(snapshot.observation.screenshot.height, 4096);
  assert_eq!(snapshot.observation.screenshot.rgba.len(), 64 * 1024 * 1024);
  assert!(
    snapshot
      .observation
      .screenshot
      .rgba
      .iter()
      .all(|pixel| *pixel == 123)
  );
}
