//! Responsive regions composed through Ratatui's constraint layout solver.
use ratatui::layout::{Constraint, Layout, Margin, Rect};

pub struct AppLayout {
  pub compact: bool,
  pub header: Rect,
  pub body: Rect,
  pub approval: Rect,
  pub composer: Rect,
  pub footer: Rect,
}

impl AppLayout {
  pub fn new(area: Rect, approval_lines: Option<usize>) -> Self {
    let compact = area.width < 70 || area.height < 16;
    let inset = if area.width >= 70 { 2 } else { 0 };
    let area = area.inner(Margin::new(inset, 0));
    let approval_height = approval_lines
      .map_or(0, |lines| {
        if compact {
          4
        } else {
          lines.saturating_add(4).clamp(6, 10) as u16
        }
      })
      .min(area.height.saturating_sub(5));
    let [header, body, approval, composer, footer] = Layout::vertical([
      Constraint::Length(if compact { 2 } else { 3 }),
      Constraint::Min(0),
      Constraint::Length(approval_height),
      Constraint::Length(if compact { 3 } else { 4 }),
      Constraint::Length(if compact { 0 } else { 2 }),
    ])
    .areas(area);
    Self {
      compact,
      header,
      body,
      approval,
      composer,
      footer,
    }
  }

  pub fn palette(&self, count: usize) -> Rect {
    let height = count
      .saturating_add(2)
      .min(usize::from(self.body.height.saturating_sub(1))) as u16;
    Rect::new(
      self.body.x,
      self
        .body
        .y
        .saturating_add(self.body.height.saturating_sub(height)),
      self.body.width.min(72),
      height,
    )
  }
}

pub struct BodyLayout {
  pub transcript: Option<Rect>,
  pub context: Option<Rect>,
}

impl BodyLayout {
  pub fn new(area: Rect, context_open: bool) -> Self {
    if !context_open {
      return Self {
        transcript: Some(area),
        context: None,
      };
    }
    if area.width < 95 {
      return Self {
        transcript: None,
        context: Some(area),
      };
    }
    let [transcript, context] =
      Layout::horizontal([Constraint::Min(30), Constraint::Length(34)]).areas(area);
    Self {
      transcript: Some(transcript),
      context: Some(context),
    }
  }
}
