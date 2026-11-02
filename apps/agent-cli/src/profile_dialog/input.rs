use super::{Focus, ProfileOperation};
use crate::{
  backend::Action,
  model::{App, Connection},
};
use agent_protocol::v1::RemoteProviderProtocol;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{Frame, style::Style};
use ratatui_textarea::TextArea;
use tokio::sync::mpsc;

#[derive(Default)]
pub struct ProfileInput {
  identity: Option<(String, u64, u64)>,
  editor: TextArea<'static>,
}
impl ProfileInput {
  pub fn sync(&mut self, app: &App) {
    let identity = app
      .profile_dialog
      .as_ref()
      .filter(|_| app.connection == Connection::Connected)
      .map(|dialog| (dialog.id.clone(), app.generation, dialog.profile.revision));
    if identity.is_none() || identity != self.identity {
      self.editor = TextArea::default();
      self.editor.set_mask_char('•');
      self.editor.set_max_histories(0);
      self.editor.set_cursor_line_style(Style::default());
      self.identity = identity;
    }
  }
  pub fn render(&self, frame: &mut Frame, app: &App, theme: crate::theme::Theme) {
    let Some(dialog) = &app.profile_dialog else {
      return;
    };
    let Some(area) = super::view::field_area(frame.area(), 4) else {
      return;
    };
    let mut editor = self.editor.clone();
    editor.set_style(Style::default().fg(theme.text).bg(theme.background));
    if dialog.focus != Focus::Credential || dialog.busy {
      editor.set_cursor_style(Style::default());
    }
    frame.render_widget(&editor, area);
  }
  pub fn handle(
    &mut self,
    app: &mut App,
    event: &Event,
    actions: &mpsc::Sender<Action>,
  ) -> anyhow::Result<Option<bool>> {
    self.sync(app);
    let Some(dialog) = app.profile_dialog.as_mut() else {
      return Ok(None);
    };
    if let Event::Key(key) = event {
      if key.kind != KeyEventKind::Press {
        return Ok(Some(false));
      }
      if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c' | 'd'))
      {
        app.profile_dialog = None;
        self.sync(app);
        return Ok(Some(true));
      }
      if key.code == KeyCode::Esc {
        if dialog.confirm_remove {
          dialog.confirm_remove = false;
        } else {
          app.profile_dialog = None;
        }
        self.sync(app);
        return Ok(Some(false));
      }
      if dialog.busy {
        return Ok(Some(false));
      }
      match key.code {
        KeyCode::Tab | KeyCode::BackTab => {
          dialog.focus = dialog
            .focus
            .next(key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT));
          dialog.confirm_remove = false;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if dialog.focus == Focus::Protocol => {
          let reverse = key.code == KeyCode::Left;
          use RemoteProviderProtocol::{AnthropicMessages, OpenaiCompletions, OpenaiResponses};
          let protocol = RemoteProviderProtocol::try_from(dialog.profile.protocol).ok();
          dialog.profile.protocol = match (protocol, reverse) {
            (Some(OpenaiResponses), true) | (Some(OpenaiCompletions), false) => AnthropicMessages,
            (Some(AnthropicMessages), true) | (Some(OpenaiResponses), false) => OpenaiCompletions,
            _ => OpenaiResponses,
          } as i32;
          dialog.changed();
        }
        KeyCode::Enter => {
          let operation = match dialog.focus {
            Focus::Close => {
              app.profile_dialog = None;
              self.sync(app);
              return Ok(Some(false));
            }
            Focus::Save | Focus::Probe => {
              let profile = dialog.snapshot();
              if profile.name.trim().is_empty()
                || profile.name.len() > 100
                || profile.base_url.is_empty()
                || profile.base_url.len() > 2048
                || profile.model.trim().is_empty()
                || profile.model.len() > 256
              {
                dialog.message =
                  "请填写名称、地址和模型；长度上限分别为 100、2048、256 字节。".into();
                return Ok(Some(false));
              }
              if dialog.profile.revision == 0 && self.editor.is_empty() {
                dialog.message = "新配置需要填写凭据。".into();
                return Ok(Some(false));
              }
              let credential = agent_secrets::SecretString::from(self.editor.lines().join(""));
              Some(if dialog.focus == Focus::Save {
                ProfileOperation::Save {
                  profile,
                  credential,
                }
              } else {
                ProfileOperation::Probe {
                  profile,
                  credential,
                }
              })
            }
            Focus::Remove if dialog.profile.revision > 0 => {
              if dialog.confirm_remove {
                Some(ProfileOperation::Remove {
                  id: dialog.profile.id.clone(),
                  revision: dialog.profile.revision,
                })
              } else {
                dialog.confirm_remove = true;
                dialog.message = format!(
                  "移除 {}（修订 {}）？再次 Enter 确认；Esc 取消。",
                  dialog.profile.id, dialog.profile.revision
                );
                None
              }
            }
            _ => {
              dialog.focus = dialog.focus.next(false);
              None
            }
          };
          if let Some(operation) = operation {
            if app.connection != Connection::Connected {
              dialog.message = "连接已断开，请重连后重新打开配置。".into();
            } else {
              crate::input::send(
                actions,
                Action::ProviderProfile {
                  generation: app.generation,
                  dialog: dialog.id.clone(),
                  operation,
                },
              )?;
              dialog.busy = true;
              dialog.message = "正在等待 daemon 确认…".into();
            }
          }
        }
        _ => {
          if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
          {
            return Ok(Some(false));
          }
          let editor = if dialog.focus == Focus::Credential {
            Some(&mut self.editor)
          } else {
            dialog
              .focus
              .editor()
              .map(|index| &mut dialog.editors[index])
          };
          if let Some(editor) = editor {
            if matches!(key.code, KeyCode::Char(c) if c.is_control()) {
              return Ok(Some(false));
            }
            let extra = match key.code {
              KeyCode::Char(c) => c.len_utf8(),
              _ => 0,
            };
            if editor.lines().iter().map(String::len).sum::<usize>() + extra
              <= agent_secrets::MAX_SECRET_BYTES
            {
              editor.input(*key);
              dialog.changed();
            }
          }
        }
      }
    } else if let Event::Paste(text) = event
      && !dialog.busy
    {
      let editor = if dialog.focus == Focus::Credential {
        Some(&mut self.editor)
      } else {
        dialog
          .focus
          .editor()
          .map(|index| &mut dialog.editors[index])
      };
      if let Some(editor) = editor {
        if text.chars().any(char::is_control)
          || editor
            .lines()
            .iter()
            .map(String::len)
            .sum::<usize>()
            .saturating_add(text.len())
            > agent_secrets::MAX_SECRET_BYTES
        {
          dialog.message = "粘贴未接受：仅支持单行，最多 16 KiB。".into();
        } else {
          editor.insert_str(text);
          dialog.changed();
        }
      }
    }
    if let Some(dialog) = &mut app.profile_dialog {
      dialog.credential_present = !self.editor.is_empty();
    }
    self.sync(app);
    Ok(Some(false))
  }
}
