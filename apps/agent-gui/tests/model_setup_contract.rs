use agent_app::{
  ManagedModelView, ModelSetupAction, ModelSetupFailureView, ModelSetupPhase, ModelSetupSource,
  ModelSetupView, RuntimePhase, RuntimeSnapshot,
};
use agent_gui::{
  model_setup::{
    ModelSetupDetailKind, ModelSetupIntent, UiLocale, present_model_setup,
    present_model_setup_for_runtime,
  },
  state::{CockpitState, ConnectionState},
};

#[test]
fn command_error_survives_byte_progress_until_the_operation_logically_changes() {
  let mut state = fresh_state();
  let mut setup = managed_setup(ModelSetupPhase::Downloading, vec![ModelSetupAction::Cancel]);
  state.apply_runtime_snapshot(&snapshot(setup.clone()));
  let ticket = state
    .begin_model_setup_command(ModelSetupIntent::Cancel)
    .unwrap();
  state.finish_model_setup_command(ticket.token, Some("cancel failed"));

  setup.downloaded_bytes = 64;
  state.apply_runtime_snapshot(&snapshot(setup));

  assert_eq!(state.model_setup_command_error(), Some("cancel failed"));
}

#[test]
fn failure_code_and_message_are_bounded_and_control_character_free() {
  let mut setup = managed_setup(ModelSetupPhase::Failed, vec![ModelSetupAction::Retry]);
  setup.failure = Some(ModelSetupFailureView {
    code: format!("bad\r\n{}", "x".repeat(700)),
    message: format!("digest\u{0000}{}", "y".repeat(700)),
    retryable: true,
  });

  let presentation = present_model_setup(&setup, UiLocale::EnUs, true, false, None);
  let failure = presentation
    .details
    .iter()
    .find(|detail| detail.kind == ModelSetupDetailKind::Failure)
    .unwrap();

  assert!(!failure.value.chars().any(char::is_control));
  assert!(failure.value.chars().count() <= 1_050);
  assert!(failure.value.contains("bad  "));
}

#[test]
fn runtime_phase_owns_the_setup_card_lifecycle() {
  let setup = managed_setup(ModelSetupPhase::Ready, vec![]);
  assert!(
    present_model_setup_for_runtime(
      RuntimePhase::ModelSetupRequired,
      Some(&setup),
      UiLocale::EnUs,
      true,
      false,
      None,
    )
    .is_some()
  );
  assert!(
    present_model_setup_for_runtime(
      RuntimePhase::Ready,
      Some(&setup),
      UiLocale::EnUs,
      true,
      false,
      None,
    )
    .is_none()
  );
}

#[test]
fn fresh_model_setup_snapshot_is_not_presented_as_pending() {
  let mut state = fresh_state();
  state.apply_runtime_snapshot(&snapshot(managed_setup(
    ModelSetupPhase::Required,
    vec![ModelSetupAction::Download],
  )));

  assert!(state.has_fresh_snapshot());
  assert!(!state.can_submit());
  assert!(
    agent_gui::view::runtime_preview_text(
      state.connection(),
      state.is_stale(),
      state.has_fresh_snapshot(),
      UiLocale::EnUs,
    )
    .contains("fresh snapshot")
  );
}

#[test]
fn every_setup_phase_has_english_and_chinese_copy() {
  let phases = [
    (ModelSetupPhase::Required, "Setup required", "需要配置"),
    (
      ModelSetupPhase::WaitingForLock,
      "Waiting for lock",
      "等待下载锁",
    ),
    (ModelSetupPhase::Checking, "Checking", "正在检查"),
    (ModelSetupPhase::Downloading, "Downloading", "正在下载"),
    (ModelSetupPhase::Verifying, "Verifying", "正在校验"),
    (ModelSetupPhase::Installing, "Installing", "正在安装"),
    (ModelSetupPhase::Activating, "Activating", "正在激活"),
    (ModelSetupPhase::Ready, "Ready", "已就绪"),
    (ModelSetupPhase::Cancelled, "Cancelled", "已取消"),
    (ModelSetupPhase::Failed, "Failed", "失败"),
  ];
  for (phase, english, chinese) in phases {
    let setup = managed_setup(phase, actions_for(phase));
    assert!(
      present_model_setup(&setup, UiLocale::EnUs, true, false, None)
        .phase
        .contains(english)
    );
    assert!(
      present_model_setup(&setup, UiLocale::ZhCn, true, false, None)
        .phase
        .contains(chinese)
    );
  }
}

#[test]
fn card_detail_order_and_disabled_control_contract_are_stable() {
  let mut setup = managed_setup(ModelSetupPhase::Failed, vec![ModelSetupAction::Retry]);
  setup.downloaded_bytes = 512;
  setup.total_bytes = Some(1024);
  setup.failure = Some(ModelSetupFailureView {
    code: "checksum_mismatch".into(),
    message: "digest mismatch".into(),
    retryable: true,
  });
  let presentation = present_model_setup(&setup, UiLocale::EnUs, false, false, Some("rpc failed"));

  assert_eq!(
    presentation
      .details
      .iter()
      .map(|detail| detail.kind)
      .collect::<Vec<_>>(),
    vec![
      ModelSetupDetailKind::Progress,
      ModelSetupDetailKind::Model,
      ModelSetupDetailKind::Operation,
      ModelSetupDetailKind::Failure,
      ModelSetupDetailKind::CommandError,
    ]
  );
  let retry = presentation.primary.unwrap();
  assert_eq!(retry.id, "model-setup-retry");
  assert!(!retry.enabled);
}

#[test]
fn blank_model_setup_command_error_uses_the_current_locale() {
  let setup = managed_setup(ModelSetupPhase::Failed, vec![ModelSetupAction::Retry]);
  let english = present_model_setup(&setup, UiLocale::EnUs, true, false, Some("\r\n"));
  let chinese = present_model_setup(&setup, UiLocale::ZhCn, true, false, Some("\r\n"));

  assert_eq!(
    english.command_error.as_deref(),
    Some("Command failed · Runtime command failed")
  );
  assert_eq!(
    chinese.command_error.as_deref(),
    Some("命令失败 · 运行时命令失败")
  );
}

fn fresh_state() -> CockpitState {
  let mut state = CockpitState::default();
  state.apply_connection(ConnectionState::Connected, None);
  state.apply_snapshot_revision("daemon-a", 1);
  state
}

fn snapshot(model_setup: ModelSetupView) -> RuntimeSnapshot {
  RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(model_setup),
    ..RuntimeSnapshot::default()
  }
}

fn actions_for(phase: ModelSetupPhase) -> Vec<ModelSetupAction> {
  match phase {
    ModelSetupPhase::Required => vec![ModelSetupAction::Download],
    ModelSetupPhase::WaitingForLock
    | ModelSetupPhase::Checking
    | ModelSetupPhase::Downloading
    | ModelSetupPhase::Verifying => vec![ModelSetupAction::Cancel],
    ModelSetupPhase::Cancelled | ModelSetupPhase::Failed => vec![ModelSetupAction::Retry],
    ModelSetupPhase::Installing | ModelSetupPhase::Activating | ModelSetupPhase::Ready => vec![],
  }
}

fn managed_setup(phase: ModelSetupPhase, allowed_actions: Vec<ModelSetupAction>) -> ModelSetupView {
  ModelSetupView {
    source: ModelSetupSource::ManagedPreset,
    phase,
    managed_model: Some(ManagedModelView {
      id: "qwen-test".into(),
      expected_download_bytes: 2,
      max_bytes: 3,
      min_available_memory_bytes: 4,
      sha256: "abc123".into(),
    }),
    operation_id: Some("operation-7".into()),
    downloaded_bytes: 0,
    total_bytes: None,
    failure: None,
    allowed_actions,
  }
}
