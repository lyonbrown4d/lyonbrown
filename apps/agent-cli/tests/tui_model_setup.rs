use agent_app::{
  AgentClientError, ManagedModelView, ModelSetupAction, ModelSetupFailureView, ModelSetupPhase,
  ModelSetupSource, ModelSetupView, RuntimeCommandError, RuntimePhase, RuntimeSnapshot,
};
use agent_cli::tui::{
  Locale, RenderContext, TuiConnectionState, TuiState, apply_remote_state, render_with_context,
};
use agent_client::{ConnectionState, RemoteState, VersionedRemoteSnapshot};
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn managed_setup_renders_confirmation_in_english_and_chinese() {
  let state = state_with(managed_setup(
    ModelSetupPhase::Required,
    vec![ModelSetupAction::Download],
  ));

  let english = screen(&state, Locale::EnUs);
  assert!(english.contains("MODEL SETUP"), "{english}");
  assert!(english.contains("model ID: qwen-test"), "{english}");
  assert!(
    english.contains("estimated download: 2.00 MiB"),
    "{english}"
  );
  assert!(english.contains("Enter/D download"), "{english}");

  let chinese = screen(&state, Locale::ZhCn);
  let compact = chinese.replace(' ', "");
  assert!(compact.contains("模型配置"), "{chinese}");
  assert!(compact.contains("模型ID:qwen-test"), "{chinese}");
  assert!(compact.contains("预计下载大小:2.00MiB"), "{chinese}");
  assert!(compact.contains("Enter/D下载"), "{chinese}");
}

#[test]
fn active_phases_and_known_or_unknown_progress_are_explicit() {
  for (phase, english, chinese) in [
    (
      ModelSetupPhase::WaitingForLock,
      "waiting for lock",
      "等待下载锁",
    ),
    (ModelSetupPhase::Checking, "checking", "正在检查"),
    (ModelSetupPhase::Downloading, "downloading", "正在下载"),
    (ModelSetupPhase::Verifying, "verifying", "正在校验"),
  ] {
    let mut setup = managed_setup(phase, vec![ModelSetupAction::Cancel]);
    setup.downloaded_bytes = 512;
    setup.total_bytes = Some(1024);
    let state = state_with(setup);
    let en = screen(&state, Locale::EnUs);
    let zh = screen(&state, Locale::ZhCn);
    let compact_zh = zh.replace(' ', "");
    assert!(en.contains(english), "{phase:?}: {en}");
    assert!(compact_zh.contains(chinese), "{phase:?}: {zh}");
    assert!(en.contains("50% · 512 B / 1.00 KiB"), "{en}");
    assert!(en.contains("[#"), "{en}");
    assert!(en.contains("operation ID: operation-7"), "{en}");
    assert!(en.contains("C/Esc cancel"), "{en}");
  }

  let mut setup = managed_setup(ModelSetupPhase::Downloading, vec![ModelSetupAction::Cancel]);
  setup.downloaded_bytes = 1536;
  setup.total_bytes = None;
  let state = state_with(setup);
  assert!(screen(&state, Locale::EnUs).contains("unknown total · downloaded 1.50 KiB"));
  assert!(
    screen(&state, Locale::ZhCn)
      .replace(' ', "")
      .contains("总量未知·已下载1.50KiB")
  );
}

#[test]
fn installing_and_activating_say_they_cannot_be_cancelled() {
  for (phase, en_text, zh_text) in [
    (
      ModelSetupPhase::Installing,
      "installing; cannot be cancelled now",
      "正在安装；当前不可取消",
    ),
    (
      ModelSetupPhase::Activating,
      "activating; cannot be cancelled now",
      "正在激活；当前不可取消",
    ),
  ] {
    let state = state_with(managed_setup(phase, vec![]));
    let en = screen(&state, Locale::EnUs);
    let zh = screen(&state, Locale::ZhCn);
    assert!(en.contains(en_text), "{en}");
    assert!(zh.replace(' ', "").contains(zh_text), "{zh}");
    assert!(!en.contains("C/Esc cancel"), "{en}");
  }
}

#[test]
fn cancelled_and_retryable_failure_render_retry_and_stable_failure_details() {
  let cancelled = state_with(managed_setup(
    ModelSetupPhase::Cancelled,
    vec![ModelSetupAction::Retry],
  ));
  assert!(screen(&cancelled, Locale::EnUs).contains("Enter/R/D retry"));

  let mut failed = managed_setup(ModelSetupPhase::Failed, vec![ModelSetupAction::Retry]);
  failed.failure = Some(ModelSetupFailureView {
    code: "checksum_mismatch".into(),
    message: "download did not match catalog digest".into(),
    retryable: true,
  });
  let failed = state_with(failed);
  let en = screen(&failed, Locale::EnUs);
  assert!(en.contains("code: checksum_mismatch"), "{en}");
  assert!(
    en.contains("message: download did not match catalog digest"),
    "{en}"
  );
  assert!(en.contains("Enter/R/D retry"), "{en}");
}

#[test]
fn configured_path_never_offers_a_download_action() {
  let mut setup = managed_setup(ModelSetupPhase::Required, vec![ModelSetupAction::Download]);
  setup.source = ModelSetupSource::ConfiguredPath;
  setup.managed_model = None;
  let state = state_with(setup);

  for locale in [Locale::EnUs, Locale::ZhCn] {
    let output = screen(&state, locale);
    assert!(output.contains("llamacpp.model_path"), "{output}");
    assert!(!output.contains("Enter/D"), "{output}");
  }
}

#[test]
fn stale_connection_keeps_setup_visible_but_disables_its_commands() {
  let mut state = state_with(managed_setup(
    ModelSetupPhase::Required,
    vec![ModelSetupAction::Download],
  ));
  state.apply_connection(
    TuiConnectionState::Reconnecting,
    Some("daemon connection lost".into()),
  );

  let output = screen(&state, Locale::EnUs);
  assert!(output.contains("model ID: qwen-test"), "{output}");
  assert!(
    output.contains("commands disabled until snapshot is fresh"),
    "{output}"
  );
  assert!(!output.contains("Enter/D download"), "{output}");
}

#[test]
fn ready_or_absent_setup_returns_to_the_conversation() {
  let ready = state_with(managed_setup(ModelSetupPhase::Ready, vec![]));
  assert!(screen(&ready, Locale::EnUs).contains("model ID: qwen-test"));

  let mut absent = TuiState::default();
  absent.sync_snapshot(&RuntimeSnapshot {
    phase: RuntimePhase::Ready,
    ..RuntimeSnapshot::default()
  });
  assert!(screen(&absent, Locale::EnUs).contains("ACTIVITY"));
}

#[test]
fn command_error_survives_sync_of_the_same_snapshot_and_is_visible() {
  let snapshot = RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(managed_setup(
      ModelSetupPhase::Required,
      vec![ModelSetupAction::Download],
    )),
    ..RuntimeSnapshot::default()
  };
  let mut state = TuiState::default();
  state.sync_snapshot(&snapshot);
  state.record_client_failure(AgentClientError::Command(
    RuntimeCommandError::ModelDownloadUnavailable,
  ));

  state.sync_snapshot(&snapshot);

  let output = screen(&state, Locale::EnUs);
  assert!(
    output.contains("command error: agent command failed"),
    "{output}"
  );
}

fn state_with(model_setup: ModelSetupView) -> TuiState {
  let mut state = TuiState::default();
  let snapshot = RuntimeSnapshot {
    phase: RuntimePhase::ModelSetupRequired,
    model_setup: Some(model_setup),
    ..RuntimeSnapshot::default()
  };
  apply_remote_state(
    &mut state,
    &RemoteState {
      connection: ConnectionState::Connected {
        server_instance_id: "daemon-test".into(),
      },
      snapshot: Some(VersionedRemoteSnapshot {
        server_instance_id: "daemon-test".into(),
        revision: 1,
        snapshot,
      }),
    },
  );
  state
}

fn managed_setup(phase: ModelSetupPhase, allowed_actions: Vec<ModelSetupAction>) -> ModelSetupView {
  ModelSetupView {
    source: ModelSetupSource::ManagedPreset,
    phase,
    managed_model: Some(ManagedModelView {
      id: "qwen-test".into(),
      expected_download_bytes: 2 * 1024 * 1024,
      max_bytes: 3 * 1024 * 1024,
      min_available_memory_bytes: 4 * 1024 * 1024,
      sha256: "abc123".into(),
    }),
    operation_id: Some("operation-7".into()),
    downloaded_bytes: 0,
    total_bytes: None,
    failure: None,
    allowed_actions,
  }
}

fn screen(state: &TuiState, locale: Locale) -> String {
  let backend = TestBackend::new(100, 22);
  let mut terminal = Terminal::new(backend).unwrap();
  terminal
    .draw(|frame| {
      render_with_context(
        frame,
        state,
        RenderContext {
          locale,
          ..RenderContext::default()
        },
      )
    })
    .unwrap();
  let buffer = terminal.backend().buffer();
  (0..buffer.area.height)
    .map(|y| {
      (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("\n")
}
