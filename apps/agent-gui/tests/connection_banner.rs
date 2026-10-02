use agent_gui::{
  copy::{UiLocale, command_error_text, composer_placeholder},
  state::ConnectionState,
  view::{
    composer_initializing_text, connection_banner_text, connection_label, runtime_preview_text,
    shell_copy,
  },
};

#[test]
fn unavailable_banner_keeps_the_actionable_transport_error_visible() {
  let text = connection_banner_text(
    ConnectionState::Unavailable,
    false,
    Some("agentd did not become ready"),
    UiLocale::EnUs,
  )
  .expect("unavailable connections need a banner");

  assert!(text.contains("UNAVAILABLE"));
  assert!(text.contains("agentd did not become ready"));
  assert!(text.contains("Retry"));
}

#[test]
fn reconnecting_banner_marks_cached_content_as_stale() {
  let text = connection_banner_text(
    ConnectionState::Reconnecting,
    true,
    Some("connection lost"),
    UiLocale::EnUs,
  )
  .expect("reconnecting connections need a banner");

  assert!(text.contains("RECONNECTING"));
  assert!(text.contains("stale"));
  assert!(text.contains("connection lost"));
}

#[test]
fn fresh_connected_state_does_not_render_a_warning_banner() {
  assert_eq!(
    connection_banner_text(ConnectionState::Connected, false, None, UiLocale::EnUs),
    None
  );
}

#[test]
fn connection_and_initialization_copy_is_localized() {
  let banner = connection_banner_text(
    ConnectionState::Reconnecting,
    true,
    Some("transport lost"),
    UiLocale::ZhCn,
  )
  .unwrap();
  assert!(banner.contains("重新连接中"));
  assert!(banner.contains("缓存内容已过期"));
  assert!(banner.contains("transport lost"));
  assert!(
    runtime_preview_text(ConnectionState::Connected, false, true, UiLocale::ZhCn)
      .contains("最新快照")
  );
  assert_eq!(
    composer_initializing_text(UiLocale::ZhCn),
    "输入框初始化中…"
  );
  assert_eq!(
    composer_initializing_text(UiLocale::EnUs),
    "Composer initializing…"
  );
  let english = shell_copy(UiLocale::EnUs);
  let chinese = shell_copy(UiLocale::ZhCn);
  assert_eq!(english.sample_task, "SAMPLE TASK");
  assert_eq!(chinese.sample_task, "示例任务");
  assert_eq!(chinese.agent_preview, "AGENT 界面预览");
  assert_eq!(chinese.transport, "传输");
  assert_eq!(chinese.compact_layout, "紧凑");
  assert_eq!(chinese.three_rail_layout, "三栏");
  assert_eq!(chinese.sessions_preview, "会话 / 预览");
  assert_eq!(chinese.gui_shell, "GUI 界面");
  assert_eq!(chinese.index_workspace, "索引工作区");
  assert_eq!(chinese.review_patches, "审查补丁");
  assert_eq!(chinese.now, "刚刚");
  assert_eq!(chinese.sample_sessions, "3 个示例会话");
  assert_eq!(
    composer_placeholder(UiLocale::EnUs),
    "Describe the next task…"
  );
  assert_eq!(composer_placeholder(UiLocale::ZhCn), "描述下一项任务…");
}

#[test]
fn every_connection_label_has_english_and_chinese_copy() {
  let labels = [
    (ConnectionState::Disconnected, "DISCONNECTED", "已断开"),
    (ConnectionState::Connecting, "CONNECTING", "连接中"),
    (ConnectionState::Connected, "CONNECTED", "已连接"),
    (ConnectionState::Reconnecting, "RECONNECTING", "重新连接中"),
    (ConnectionState::Unavailable, "UNAVAILABLE", "不可用"),
    (ConnectionState::Incompatible, "INCOMPATIBLE", "不兼容"),
  ];
  for (state, english, chinese) in labels {
    assert_eq!(connection_label(state, UiLocale::EnUs), english);
    assert_eq!(connection_label(state, UiLocale::ZhCn), chinese);
  }
}

#[test]
fn blank_command_error_fallback_tracks_the_current_locale() {
  assert_eq!(
    command_error_text("", UiLocale::EnUs),
    "Runtime command failed"
  );
  assert_eq!(command_error_text("", UiLocale::ZhCn), "运行时命令失败");
  assert_eq!(
    command_error_text("rpc failed", UiLocale::ZhCn),
    "rpc failed"
  );
}
