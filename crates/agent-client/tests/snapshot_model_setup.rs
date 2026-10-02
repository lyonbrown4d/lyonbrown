use agent_app::{
  ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView, RuntimeSnapshot,
};
use agent_client::{SnapshotDecodeError, decode_snapshot};
use agent_protocol::PROTOCOL_VERSION;
use agent_protocol::v1::{
  ManagedModel, ModelSelection, ModelSetupAction as WireAction, ModelSetupFailure,
  ModelSetupPhase as WirePhase, ModelSetupSnapshot, ModelSetupSource as WireSource, RuntimePhase,
  RuntimeSnapshot as WireSnapshot,
};

fn base_snapshot() -> WireSnapshot {
  WireSnapshot {
    protocol_version: PROTOCOL_VERSION,
    revision: 1,
    phase: RuntimePhase::ModelSetupRequired as i32,
    model_selection: Some(ModelSelection {
      selection: Some(agent_protocol::v1::model_selection::Selection::Auto(true)),
    }),
    ..WireSnapshot::default()
  }
}

fn managed_setup() -> ModelSetupSnapshot {
  ModelSetupSnapshot {
    source: WireSource::ManagedPreset as i32,
    phase: WirePhase::Downloading as i32,
    managed_model: Some(ManagedModel {
      id: "qwen3-1.7b-q4_k_m".into(),
      expected_download_bytes: 1_000,
      max_bytes: 1_100,
      min_available_memory_bytes: 2_000,
      sha256: "ab".repeat(32),
    }),
    operation_id: Some("download-1".into()),
    downloaded_bytes: 400,
    total_bytes: Some(1_000),
    failure: None,
    allowed_actions: vec![WireAction::Cancel as i32],
  }
}

#[test]
fn absent_model_setup_is_backward_compatible() {
  let decoded = decode_snapshot(base_snapshot()).unwrap();

  assert_eq!(decoded.snapshot.model_setup, None);
}

#[test]
fn managed_model_setup_decodes_into_transport_independent_view() {
  let mut wire = base_snapshot();
  wire.model_setup = Some(managed_setup());

  let decoded = decode_snapshot(wire).unwrap();

  assert_eq!(
    decoded.snapshot.model_setup,
    Some(ModelSetupView {
      source: ModelSetupSource::ManagedPreset,
      phase: ModelSetupPhase::Downloading,
      managed_model: Some(agent_app::ManagedModelView {
        id: "qwen3-1.7b-q4_k_m".into(),
        expected_download_bytes: 1_000,
        max_bytes: 1_100,
        min_available_memory_bytes: 2_000,
        sha256: "ab".repeat(32),
      }),
      operation_id: Some("download-1".into()),
      downloaded_bytes: 400,
      total_bytes: Some(1_000),
      failure: None,
      allowed_actions: vec![ModelSetupAction::Cancel],
    })
  );
}

#[test]
fn invalid_model_setup_progress_is_rejected() {
  let mut setup = managed_setup();
  setup.downloaded_bytes = 1_001;
  let mut wire = base_snapshot();
  wire.model_setup = Some(setup);

  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelSetup(_))
  ));
}

#[test]
fn unknown_model_setup_phase_is_rejected() {
  let mut setup = managed_setup();
  setup.phase = 999;
  let mut wire = base_snapshot();
  wire.model_setup = Some(setup);

  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::UnknownEnum {
      field: "model_setup.phase",
      value: 999
    })
  ));
}

#[test]
fn managed_source_requires_managed_model_metadata() {
  let mut setup = managed_setup();
  setup.managed_model = None;
  let mut wire = base_snapshot();
  wire.model_setup = Some(setup);

  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::Missing("model_setup.managed_model"))
  ));
}

#[test]
fn managed_model_identity_and_digest_are_strictly_validated() {
  for mutate in [
    |model: &mut ManagedModel| model.id.clear(),
    |model: &mut ManagedModel| model.id = "x".repeat(129),
    |model: &mut ManagedModel| model.sha256 = "ab".repeat(31),
    |model: &mut ManagedModel| model.sha256 = "z".repeat(64),
  ] {
    let mut setup = managed_setup();
    mutate(setup.managed_model.as_mut().unwrap());
    let mut wire = base_snapshot();
    wire.model_setup = Some(setup);

    assert!(matches!(
      decode_snapshot(wire),
      Err(SnapshotDecodeError::InvalidModelSetup(_))
    ));
  }
}

#[test]
fn managed_model_size_limits_are_strictly_validated() {
  let invalid_sizes = [
    (1_000, 0, 0, None),
    (1_101, 1_100, 0, None),
    (1_000, 1_100, 1_101, None),
    (1_000, 1_100, 400, Some(1_101)),
  ];
  for (expected, max, downloaded, total) in invalid_sizes {
    let mut setup = managed_setup();
    let model = setup.managed_model.as_mut().unwrap();
    model.expected_download_bytes = expected;
    model.max_bytes = max;
    setup.downloaded_bytes = downloaded;
    setup.total_bytes = total;
    let mut wire = base_snapshot();
    wire.model_setup = Some(setup);

    assert!(matches!(
      decode_snapshot(wire),
      Err(SnapshotDecodeError::InvalidModelSetup(_))
    ));
  }
}

#[test]
fn active_model_setup_requires_a_non_empty_operation_id() {
  for operation_id in [None, Some(" ".into())] {
    let mut setup = managed_setup();
    setup.operation_id = operation_id;
    let mut wire = base_snapshot();
    wire.model_setup = Some(setup);

    assert!(matches!(
      decode_snapshot(wire),
      Err(SnapshotDecodeError::InvalidModelSetup(_))
    ));
  }
}

#[test]
fn failure_presence_must_match_failed_phase() {
  let mut missing_failure = managed_setup();
  missing_failure.phase = WirePhase::Failed as i32;
  missing_failure.operation_id = Some("download-1".into());
  missing_failure.allowed_actions.clear();
  let mut wire = base_snapshot();
  wire.model_setup = Some(missing_failure);
  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelSetup(_))
  ));

  let mut unexpected_failure = managed_setup();
  unexpected_failure.failure = Some(ModelSetupFailure {
    code: "network".into(),
    message: "failed".into(),
    retryable: true,
  });
  let mut wire = base_snapshot();
  wire.model_setup = Some(unexpected_failure);
  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelSetup(_))
  ));
}

#[test]
fn allowed_actions_must_match_the_current_phase() {
  let mut ready_with_cancel = managed_setup();
  ready_with_cancel.phase = WirePhase::Ready as i32;
  ready_with_cancel.operation_id = None;
  let mut wire = base_snapshot();
  wire.model_setup = Some(ready_with_cancel);
  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelSetup(_))
  ));

  let mut duplicate_cancel = managed_setup();
  duplicate_cancel
    .allowed_actions
    .push(WireAction::Cancel as i32);
  let mut wire = base_snapshot();
  wire.model_setup = Some(duplicate_cancel);
  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelSetup(_))
  ));
}

#[test]
fn configured_path_rejects_managed_model_metadata() {
  let mut setup = managed_setup();
  setup.source = WireSource::ConfiguredPath as i32;
  let mut wire = base_snapshot();
  wire.model_setup = Some(setup);

  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelSetup(_))
  ));
}

#[test]
fn operation_id_is_bounded_and_required_for_cancelled_phase() {
  let mut oversized = managed_setup();
  oversized.operation_id = Some("x".repeat(129));
  let mut wire = base_snapshot();
  wire.model_setup = Some(oversized);
  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelSetup(_))
  ));

  let mut cancelled = managed_setup();
  cancelled.phase = WirePhase::Cancelled as i32;
  cancelled.operation_id = None;
  cancelled.allowed_actions = vec![WireAction::Retry as i32];
  let mut wire = base_snapshot();
  wire.model_setup = Some(cancelled);
  assert!(matches!(
    decode_snapshot(wire),
    Err(SnapshotDecodeError::InvalidModelSetup(_))
  ));
}

#[test]
fn runtime_snapshot_default_keeps_model_setup_absent() {
  assert_eq!(RuntimeSnapshot::default().model_setup, None);
}
