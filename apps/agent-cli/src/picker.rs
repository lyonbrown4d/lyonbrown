//! Overlay selector for command results that need a choice.
//!
//! Selection is a transient composer overlay: it never owns daemon authority and never becomes a
//! navigation route. Confirming an item only produces an `Action` for the caller to dispatch.

use agent_protocol::v1::ModelCandidate;
use ratatui::{
  Frame,
  layout::Rect,
  style::{Modifier, Style},
  text::{Line, Span, Text},
  widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use ratatui_interact::components::{ListPicker, ListPickerState, ListPickerStyle};

use crate::theme::Theme;

/// What confirming the highlighted row dispatches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerValue {
  Model {
    model: String,
    execution: String,
  },
  Session {
    id: String,
  },
  /// Confirming uploads the detected credential environment for this provider.
  Provider {
    provider: String,
    environment: String,
  },
  Skill {
    name: String,
    scope: String,
  },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerItem {
  pub label: String,
  pub detail: String,
  pub value: PickerValue,
}

/// `ListPicker::new` requires `Display`; rows render through `render_item` instead.
impl std::fmt::Display for PickerItem {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(formatter, "{}", self.label)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
  pub title: String,
  pub items: Vec<PickerItem>,
  pub selected: usize,
  pub loading: bool,
  pub error: String,
}

impl Picker {
  pub fn models() -> Self {
    Self::loading("选择模型 · ↑↓ 选择 · Enter 确认 · Esc 取消")
  }

  /// Shown while the daemon catalog request is in flight.
  pub fn loading(title: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      items: Vec::new(),
      selected: 0,
      loading: true,
      error: String::new(),
    }
  }

  /// One row per model execution, so a model with several runtimes stays unambiguous.
  pub fn with_models(mut self, models: &[ModelCandidate]) -> Self {
    self.loading = false;
    self.items = models
      .iter()
      .flat_map(|model| {
        let current = model.selected;
        model
          .executions
          .iter()
          .map(move |execution| PickerItem {
            label: format!("{} · {}", model.display_name, execution.label),
            detail: format!(
              "{} · {} · {}",
              model.vendor,
              execution.transport,
              model_state(&model.state, &execution.reason)
            ),
            value: PickerValue::Model {
              model: model.id.clone(),
              execution: execution.id.clone(),
            },
          })
          .map(|item| (current, item))
          .collect::<Vec<_>>()
      })
      .enumerate()
      .map(|(index, (selected, mut item))| {
        if selected && index == 0 {
          item.detail = format!("当前 · {}", item.detail);
        }
        item
      })
      .collect();
    self
  }

  /// Session switching keeps the F2 panel for browsing; this picker stays a quick jump.
  pub fn sessions(sessions: &[crate::model::Session], current: Option<&str>) -> Self {
    Self {
      title: "切换会话 · ↑↓ 选择 · Enter 确认 · Esc 取消".into(),
      items: sessions
        .iter()
        .map(|session| PickerItem {
          label: session.title.clone(),
          detail: if current == Some(session.id.as_str()) {
            "当前".to_owned()
          } else {
            String::new()
          },
          value: PickerValue::Session {
            id: session.id.clone(),
          },
        })
        .collect(),
      selected: sessions
        .iter()
        .position(|session| current == Some(session.id.as_str()))
        .unwrap_or(0),
      loading: false,
      error: String::new(),
    }
  }

  /// One row per provider; only a provider with a detected variable is worth confirming.
  pub fn providers(
    providers: &[agent_protocol::v1::Provider],
    candidates: &[crate::resource_command::ClientProviderCandidate],
  ) -> Self {
    Self {
      title: "选择提供方 · ↑↓ 选择 · Enter 上传凭据 · Esc 取消".into(),
      items: providers
        .iter()
        .map(|provider| {
          // Only an allowlisted variable that this client actually detects can be uploaded.
          let environment = agent_config::provider_definition(&provider.id)
            .and_then(|definition| {
              definition
                .credential_env_names
                .iter()
                .find(|name| {
                  candidates
                    .iter()
                    .any(|candidate| candidate.environment == **name)
                })
                .map(|name| (*name).to_owned())
            })
            .or_else(|| {
              (!provider.credential_environment.is_empty())
                .then(|| provider.credential_environment.clone())
            })
            .unwrap_or_default();
          PickerItem {
            label: provider.name.clone(),
            detail: if provider.credential_state == "configured" {
              format!("已配置 · {}", provider.credential_state)
            } else if environment.is_empty() {
              "未检测到凭据环境变量".to_owned()
            } else {
              format!("可上传 {environment}")
            },
            value: PickerValue::Provider {
              provider: provider.id.clone(),
              environment,
            },
          }
        })
        .collect(),
      selected: 0,
      loading: false,
      error: String::new(),
    }
  }

  pub fn skills(skills: &[agent_protocol::v1::Skill]) -> Self {
    Self {
      title: "选择技能 · ↑↓ 选择 · Enter 查看 · Esc 取消".into(),
      items: skills
        .iter()
        .map(|skill| PickerItem {
          label: skill.name.clone(),
          detail: format!(
            "{} · {} 文件 · {}",
            skill.scope,
            skill.files,
            if skill.enabled {
              "已启用"
            } else {
              "未启用"
            }
          ),
          value: PickerValue::Skill {
            name: skill.name.clone(),
            scope: skill.scope.clone(),
          },
        })
        .collect(),
      selected: 0,
      loading: false,
      error: String::new(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.items.is_empty()
  }

  pub fn selected_value(&self) -> Option<&PickerValue> {
    self.items.get(self.selected).map(|item| &item.value)
  }

  pub fn next(&mut self) {
    if !self.items.is_empty() {
      self.selected = (self.selected + 1).min(self.items.len() - 1);
    }
  }

  pub fn previous(&mut self) {
    self.selected = self.selected.saturating_sub(1);
  }

  /// Rows shown by the overlay; the caller sizes the popup with the same number.
  pub fn visible_rows(&self) -> usize {
    self.items.len().clamp(1, 8)
  }
}

fn model_state(state: &str, reason: &str) -> String {
  match state {
    "ready" => "可用".into(),
    "unavailable" => "不可用".into(),
    _ if reason.is_empty() => "需要凭据".into(),
    _ => format!("需要凭据 · {reason}"),
  }
}

pub fn render(frame: &mut Frame, area: Rect, picker: &Picker, theme: Theme) {
  frame.render_widget(Clear, area);
  if picker.items.is_empty() {
    let message = if picker.loading {
      "加载中…"
    } else {
      "没有可选项"
    };
    frame.render_widget(
      Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
          picker.title.clone(),
          Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(message, Style::default().fg(theme.muted))),
      ]))
      .block(
        Block::default()
          .borders(Borders::ALL)
          .border_style(Style::default().fg(theme.accent)),
      )
      .wrap(Wrap { trim: false }),
      area,
    );
    return;
  }
  let style = ListPickerStyle {
    selected_style: Style::default()
      .fg(theme.accent)
      .add_modifier(Modifier::BOLD),
    normal_style: Style::default().fg(theme.text),
    indicator_style: Style::default().fg(theme.accent),
    border_style: Style::default().fg(theme.accent),
    indicator: "› ",
    indicator_empty: "  ",
    bordered: true,
  };
  let mut state = ListPickerState::new(picker.items.len());
  state.select(picker.selected);
  state.ensure_visible(area.height.saturating_sub(3) as usize);
  let items = picker.items.clone();
  frame.render_widget(
    ListPicker::new(&items, &state)
      .render_item(|item, _index, _selected| {
        vec![Line::from(vec![
          Span::raw(format!("{:<28}", item.label)),
          Span::styled(item.detail.clone(), Style::default().fg(theme.muted)),
        ])]
      })
      .title(&picker.title)
      .style(style),
    area,
  );
}

#[cfg(test)]
mod tests {
  use super::*;
  use agent_protocol::v1::ModelExecution;

  fn candidate(selected: bool) -> ModelCandidate {
    ModelCandidate {
      id: "deepseek-chat".into(),
      display_name: "DeepSeek Chat".into(),
      vendor: "deepseek".into(),
      source: "builtin".into(),
      state: "configurable".into(),
      selected,
      executions: vec![ModelExecution {
        id: "builtin:deepseek-chat".into(),
        label: "API".into(),
        transport: "api".into(),
        state: "configurable".into(),
        reason: String::new(),
      }],
    }
  }

  #[test]
  fn builds_one_row_per_execution_and_marks_the_current_model() {
    let picker = Picker::models().with_models(&[candidate(true), candidate(false)]);
    assert!(!picker.loading);
    assert_eq!(picker.items.len(), 2);
    assert!(picker.items[0].detail.starts_with("当前"));
    assert_eq!(
      picker.items[0].value,
      PickerValue::Model {
        model: "deepseek-chat".into(),
        execution: "builtin:deepseek-chat".into(),
      }
    );
  }

  #[test]
  fn sessions_picker_marks_the_current_session() {
    let sessions = vec![
      crate::model::Session {
        id: "a".into(),
        title: "A".into(),
      },
      crate::model::Session {
        id: "b".into(),
        title: "B".into(),
      },
    ];
    let picker = Picker::sessions(&sessions, Some("b"));
    assert_eq!(picker.selected, 1);
    assert_eq!(picker.items[1].detail, "当前");
    assert_eq!(picker.items[0].detail, "");
    assert_eq!(
      picker.items[0].value,
      PickerValue::Session { id: "a".into() }
    );
  }

  #[test]
  fn provider_picker_reports_a_uploadable_environment() {
    use crate::resource_command::ClientProviderCandidate;
    let providers = vec![agent_protocol::v1::Provider {
      id: "openai".into(),
      name: "OpenAI".into(),
      credential_state: "missing".into(),
      ..Default::default()
    }];
    let candidates = vec![ClientProviderCandidate {
      environment: "OPENAI_API_KEY".into(),
      state: "detected".into(),
    }];
    let picker = Picker::providers(&providers, &candidates);
    assert_eq!(picker.items[0].label, "OpenAI");
    assert_eq!(picker.items[0].detail, "可上传 OPENAI_API_KEY");
    assert_eq!(
      picker.items[0].value,
      PickerValue::Provider {
        provider: "openai".into(),
        environment: "OPENAI_API_KEY".into()
      }
    );
  }

  #[test]
  fn skill_picker_reports_scope_and_enabled_state() {
    let skills = vec![agent_protocol::v1::Skill {
      name: "review".into(),
      scope: "daemon_user".into(),
      files: 3,
      enabled: true,
      ..Default::default()
    }];
    let picker = Picker::skills(&skills);
    assert_eq!(picker.items[0].label, "review");
    assert!(picker.items[0].detail.contains("daemon_user"));
    assert!(picker.items[0].detail.contains("已启用"));
  }

  #[test]
  fn selection_stays_inside_the_item_range() {
    let mut picker = Picker::models().with_models(&[candidate(false)]);
    picker.next();
    picker.next();
    assert_eq!(picker.selected, 0);
    picker.previous();
    assert_eq!(picker.selected, 0);
  }
}
