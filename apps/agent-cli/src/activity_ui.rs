//! Keep selected execution records visible using the same wrapping as the View.
use crate::{config::ThemeChoice, model::App, theme::Theme};
use ratatui::{
  layout::Rect,
  text::Text,
  widgets::{Paragraph, Wrap},
};
pub(crate) fn focus(app: &mut App) {
  let area = Rect::new(0, 0, app.viewport_width, app.viewport);
  let Some(context) = crate::layout::BodyLayout::new(area, true).context else {
    return;
  };
  let text = app.activity.text(Theme::resolve(ThemeChoice::Dark, None));
  let prefix = Text::from(
    text
      .lines
      .into_iter()
      .take(app.activity.selected_line())
      .collect::<Vec<_>>(),
  );
  let offset = Paragraph::new(prefix)
    .wrap(Wrap { trim: false })
    .line_count(context.width.saturating_sub(1));
  app.router.set_scroll(offset.min(u16::MAX as usize) as u16);
}
