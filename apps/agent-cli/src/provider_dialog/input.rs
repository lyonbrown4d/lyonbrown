//! Own the mature masked editor outside persistent presentation models.
use super::{CredentialChange, Focus};
use crate::{
  backend::Action,
  model::{App, Connection},
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{Frame, style::Style};
use ratatui_textarea::TextArea;
use tokio::sync::mpsc;

#[derive(Default)]
pub struct CredentialInput {
  identity: Option<(String, u64, u64)>,
  editor: TextArea<'static>,
}
impl CredentialInput {
  /// Clear credentials on close, acknowledged mutation, node/session change or disconnect.
  pub fn sync(&mut self, app: &App) {
    let identity = app
      .provider_dialog
      .as_ref()
      .filter(|_| app.connection == Connection::Connected)
      .map(|dialog| (dialog.id.clone(), app.generation, dialog.revision));
    if identity.is_none() || identity != self.identity {
      self.editor = TextArea::default();
      self.editor.set_mask_char('•');
      self.editor.set_cursor_line_style(Style::default());
      self.editor.set_max_histories(0);
      self.identity = identity;
    }
  }
  pub fn render(&self, frame: &mut Frame, app: &App, theme: crate::theme::Theme) {
    let Some(dialog) = &app.provider_dialog else {
      return;
    };
    if !dialog.editable() {
      return;
    }
    if let Some(area) = super::view::credential_area(frame.area()) {
      let mut editor = self.editor.clone();
      editor.set_style(Style::default().fg(theme.text).bg(theme.background));
      if dialog.focus != Focus::Credential || dialog.busy {
        editor.set_cursor_style(Style::default());
      }
      frame.render_widget(&editor, area);
    }
  }
  /// Returns None when the regular input handler should own the event.
  pub fn handle(
    &mut self,
    app: &mut App,
    event: &Event,
    actions: &mpsc::Sender<Action>,
  ) -> anyhow::Result<Option<bool>> {
    self.sync(app);
    let Some(dialog) = app.provider_dialog.as_mut() else {
      return Ok(None);
    };
    if let Event::Key(key) = event {
      if key.kind != KeyEventKind::Press {
        return Ok(Some(false));
      }
      if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c' | 'd'))
      {
        app.provider_dialog = None;
        self.sync(app);
        return Ok(Some(true));
      }
      if key.code == KeyCode::Esc {
        if dialog.confirm_remove {
          dialog.confirm_remove = false;
          dialog.message = "已取消移除。".into();
        } else {
          app.provider_dialog = None;
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
        KeyCode::Enter => {
          let change = match dialog.focus {
            Focus::Credential => {
              dialog.focus = Focus::Save;
              None
            }
            Focus::Close => {
              app.provider_dialog = None;
              self.sync(app);
              return Ok(Some(false));
            }
            Focus::Save if dialog.editable() => {
              if self.editor.is_empty() {
                dialog.message = "请输入新密钥；留空不会覆盖已有配置。".into();
                None
              } else {
                Some(CredentialChange::Typed(agent_secrets::SecretString::from(
                  self.editor.lines().join(""),
                )))
              }
            }
            Focus::Import if dialog.editable() && !dialog.environment.is_empty() => {
              Some(CredentialChange::Environment(dialog.environment.clone()))
            }
            Focus::Remove if dialog.editable() => {
              if dialog.confirm_remove {
                Some(CredentialChange::Remove)
              } else {
                dialog.confirm_remove = true;
                dialog.message =
                  "确认移除 daemon 保存的凭据？再次按 Enter；Esc 返回。环境变量不受影响。".into();
                None
              }
            }
            _ => {
              dialog.message = "此操作当前不可用。".into();
              None
            }
          };
          if let Some(change) = change {
            if app.connection != Connection::Connected {
              dialog.message = "连接已断开，请关闭对话框并重连。".into();
            } else {
              crate::input::send(
                actions,
                Action::ProviderCredential {
                  provider: dialog.provider.id.clone(),
                  change,
                  generation: app.generation,
                  dialog: dialog.id.clone(),
                },
              )?;
              dialog.busy = true;
              dialog.message = "正在等待 agentd 确认…".into();
            }
          }
        }
        _ if dialog.focus == Focus::Credential && dialog.editable() => {
          // Credential input is single-line, bounded, and cannot copy/cut to a clipboard.
          if matches!(key.code, KeyCode::Char(c) if c.is_control())
            || (matches!(key.code, KeyCode::Char(_))
              && key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT))
          {
            return Ok(Some(false));
          }
          let extra = match key.code {
            KeyCode::Char(c) => c.len_utf8(),
            _ => 0,
          };
          if self
            .editor
            .lines()
            .iter()
            .map(String::len)
            .sum::<usize>()
            .saturating_add(extra)
            <= agent_secrets::MAX_SECRET_BYTES
          {
            self.editor.input(*key);
          }
        }
        _ => {}
      }
    } else if let Event::Paste(text) = event
      && !dialog.busy
      && dialog.focus == Focus::Credential
      && dialog.editable()
    {
      let size: usize = self.editor.lines().iter().map(String::len).sum();
      if text.chars().any(char::is_control)
        || size.saturating_add(text.len()) > agent_secrets::MAX_SECRET_BYTES
      {
        dialog.message = "粘贴未接受：密钥必须为单行，且不超过 16 KiB。".into();
      } else {
        self.editor.insert_str(text);
      }
    }
    if let Some(dialog) = &mut app.provider_dialog {
      dialog.credential_present = !self.editor.is_empty();
    }
    self.sync(app);
    Ok(Some(false))
  }
}
