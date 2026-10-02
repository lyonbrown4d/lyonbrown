use agent_app::{
  ModelSetupAction, ModelSetupPhase, ModelSetupSource, ModelSetupView, RuntimePhase,
};

use crate::copy::{Copy, UiLocale, command_error_text};

use super::{ModelSetupIntent, sanitize_command_error, sanitize_display_text};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelSetupDetailKind {
  Progress,
  Model,
  Operation,
  Failure,
  CommandError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSetupDetail {
  pub kind: ModelSetupDetailKind,
  pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSetupControl {
  pub id: &'static str,
  pub label: &'static str,
  pub intent: ModelSetupIntent,
  pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSetupPresentation {
  pub title: &'static str,
  pub source: String,
  pub phase: String,
  pub progress: Option<String>,
  pub failure: Option<String>,
  pub command_error: Option<String>,
  pub model_id: Option<String>,
  pub operation_id: Option<String>,
  pub primary: Option<ModelSetupControl>,
  pub cancel: Option<ModelSetupControl>,
  pub details: Vec<ModelSetupDetail>,
}

pub fn present_model_setup_for_runtime(
  runtime_phase: RuntimePhase,
  setup: Option<&ModelSetupView>,
  locale: UiLocale,
  commands_ready: bool,
  pending: bool,
  command_error: Option<&str>,
) -> Option<ModelSetupPresentation> {
  if runtime_phase != RuntimePhase::ModelSetupRequired {
    return None;
  }
  Some(present_model_setup(
    setup?,
    locale,
    commands_ready,
    pending,
    command_error,
  ))
}

pub fn present_model_setup(
  setup: &ModelSetupView,
  locale: UiLocale,
  commands_ready: bool,
  pending: bool,
  command_error: Option<&str>,
) -> ModelSetupPresentation {
  let copy = Copy::new(locale);
  let controls_enabled = commands_ready && !pending;
  let progress = progress_text(setup, copy);
  let failure = setup.failure.as_ref().map(|failure| {
    let code = failure_value_text(&failure.code, copy);
    let message = failure_value_text(&failure.message, copy);
    format!(
      "{} {} · {} {}",
      copy.text("Code", "代码"),
      code,
      copy.text("Message", "信息"),
      message
    )
  });
  let command_error = command_error.map(|error| {
    let error = sanitize_command_error(error);
    format!(
      "{} · {}",
      copy.text("Command failed", "命令失败"),
      command_error_text(&error, locale)
    )
  });
  let model_id = setup.managed_model.as_ref().map(|model| model.id.clone());
  let operation_id = setup.operation_id.clone();
  let details = ordered_details(
    progress.as_ref(),
    model_id.as_ref(),
    operation_id.as_ref(),
    failure.as_ref(),
    command_error.as_ref(),
  );
  ModelSetupPresentation {
    title: copy.text("MODEL SETUP", "模型配置"),
    source: source_text(setup.source, copy).to_owned(),
    phase: format!(
      "{} · {}",
      copy.text("Phase", "阶段"),
      phase_text(setup.phase, copy)
    ),
    progress,
    failure,
    command_error,
    model_id,
    operation_id,
    primary: primary_control(setup, copy, controls_enabled),
    cancel: cancel_control(setup, copy, controls_enabled),
    details,
  }
}

fn failure_value_text(value: &str, copy: Copy) -> String {
  let value = sanitize_display_text(value);
  if value.is_empty() {
    copy.text("unknown", "未知").to_owned()
  } else {
    value
  }
}

fn ordered_details(
  progress: Option<&String>,
  model_id: Option<&String>,
  operation_id: Option<&String>,
  failure: Option<&String>,
  command_error: Option<&String>,
) -> Vec<ModelSetupDetail> {
  [
    (ModelSetupDetailKind::Progress, progress),
    (ModelSetupDetailKind::Model, model_id),
    (ModelSetupDetailKind::Operation, operation_id),
    (ModelSetupDetailKind::Failure, failure),
    (ModelSetupDetailKind::CommandError, command_error),
  ]
  .into_iter()
  .filter_map(|(kind, value)| {
    value.map(|value| ModelSetupDetail {
      kind,
      value: value.clone(),
    })
  })
  .collect()
}

fn primary_control(setup: &ModelSetupView, copy: Copy, enabled: bool) -> Option<ModelSetupControl> {
  if setup.source == ModelSetupSource::ConfiguredPath || setup.managed_model.is_none() {
    return None;
  }
  if setup.allowed_actions.contains(&ModelSetupAction::Download) {
    return Some(ModelSetupControl {
      id: "model-setup-download",
      label: copy.text("Download model", "下载模型"),
      intent: ModelSetupIntent::Start,
      enabled,
    });
  }
  if setup.allowed_actions.contains(&ModelSetupAction::Retry) {
    return Some(ModelSetupControl {
      id: "model-setup-retry",
      label: copy.text("Retry download", "重试下载"),
      intent: ModelSetupIntent::Retry,
      enabled,
    });
  }
  None
}

fn cancel_control(setup: &ModelSetupView, copy: Copy, enabled: bool) -> Option<ModelSetupControl> {
  if setup.source == ModelSetupSource::ConfiguredPath
    || setup.operation_id.is_none()
    || !setup.allowed_actions.contains(&ModelSetupAction::Cancel)
    || matches!(
      setup.phase,
      ModelSetupPhase::Installing | ModelSetupPhase::Activating
    )
  {
    return None;
  }
  Some(ModelSetupControl {
    id: "model-setup-cancel",
    label: copy.text("Cancel download", "取消下载"),
    intent: ModelSetupIntent::Cancel,
    enabled,
  })
}

fn source_text(source: ModelSetupSource, copy: Copy) -> &'static str {
  match source {
    ModelSetupSource::ManagedPreset => copy.text("Managed local model", "托管本地模型"),
    ModelSetupSource::ConfiguredPath => copy.text(
      "Configured path · check llamacpp.model_path",
      "已配置路径 · 请检查 llamacpp.model_path",
    ),
  }
}

fn phase_text(phase: ModelSetupPhase, copy: Copy) -> &'static str {
  match phase {
    ModelSetupPhase::Required => copy.text("Setup required", "需要配置"),
    ModelSetupPhase::WaitingForLock => copy.text("Waiting for lock", "等待下载锁"),
    ModelSetupPhase::Checking => copy.text("Checking", "正在检查"),
    ModelSetupPhase::Downloading => copy.text("Downloading", "正在下载"),
    ModelSetupPhase::Verifying => copy.text("Verifying", "正在校验"),
    ModelSetupPhase::Installing => copy.text("Installing · cannot cancel", "正在安装 · 无法取消"),
    ModelSetupPhase::Activating => copy.text("Activating · cannot cancel", "正在激活 · 无法取消"),
    ModelSetupPhase::Ready => copy.text("Ready", "已就绪"),
    ModelSetupPhase::Cancelled => copy.text("Cancelled", "已取消"),
    ModelSetupPhase::Failed => copy.text("Failed", "失败"),
  }
}

fn progress_text(setup: &ModelSetupView, copy: Copy) -> Option<String> {
  let visible = setup.downloaded_bytes > 0
    || setup.total_bytes.is_some()
    || matches!(
      setup.phase,
      ModelSetupPhase::WaitingForLock
        | ModelSetupPhase::Checking
        | ModelSetupPhase::Downloading
        | ModelSetupPhase::Verifying
    );
  if !visible {
    return None;
  }
  match setup.total_bytes {
    Some(total) if total > 0 => {
      let percent = (u128::from(setup.downloaded_bytes) * 100 / u128::from(total)).min(100);
      Some(format!(
        "{percent}% · {} / {}",
        format_bytes(setup.downloaded_bytes),
        format_bytes(total)
      ))
    }
    _ => Some(format!(
      "{} · {} {}",
      copy.text("Unknown total", "总量未知"),
      format_bytes(setup.downloaded_bytes),
      copy.text("downloaded", "已下载")
    )),
  }
}

fn format_bytes(bytes: u64) -> String {
  const KIB: u64 = 1024;
  const MIB: u64 = KIB * 1024;
  const GIB: u64 = MIB * 1024;
  if bytes < KIB {
    return format!("{bytes} B");
  }
  let (value, unit) = if bytes < MIB {
    (bytes as f64 / KIB as f64, "KiB")
  } else if bytes < GIB {
    (bytes as f64 / MIB as f64, "MiB")
  } else {
    (bytes as f64 / GIB as f64, "GiB")
  };
  format!("{value:.2} {unit}")
}
