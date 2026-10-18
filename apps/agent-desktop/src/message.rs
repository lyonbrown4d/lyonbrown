use gpui_kit::component::text::TextViewState;
use gpui_kit::*;

pub struct ChatMessage {
  pub role: SharedString,
  pub markdown: Entity<TextViewState>,
}

impl ChatMessage {
  pub fn new(role: impl Into<SharedString>, body: impl Into<SharedString>, cx: &mut App) -> Self {
    let body = body.into();
    Self {
      role: role.into(),
      markdown: cx.new(|cx| TextViewState::markdown(body.as_ref(), cx)),
    }
  }

  pub fn append(&self, text: &str, cx: &mut App) {
    self
      .markdown
      .update(cx, |state, cx| state.push_str(text, cx));
  }
}
