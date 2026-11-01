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

/// Completion rows are bounded so the popup never becomes a full-screen list.
const PALETTE_MAX_ROWS: usize = 8;
const PALETTE_MAX_WIDTH: u16 = 72;

pub struct PaletteLayout {
  pub area: Rect,
  pub rows: usize,
  pub has_more: bool,
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

  /// Slash completion floats directly above the composer instead of taking a panel column.
  pub fn palette(&self, count: usize) -> PaletteLayout {
    let space = self.composer.y.saturating_sub(self.body.y);
    // Align the popup with the composer text, not with its `❯ ` prompt column.
    let left = self.composer.x.saturating_add(2).min(self.body.right());
    let width = self
      .composer
      .width
      .saturating_sub(2)
      .min(PALETTE_MAX_WIDTH)
      .min(self.body.right().saturating_sub(left));
    let room = usize::from(self.body.height / 2).max(3);
    let mut rows = count.clamp(1, PALETTE_MAX_ROWS.min(room));
    loop {
      let has_more = count > rows;
      let height = rows as u16 + 2 + u16::from(has_more);
      if height <= space || rows == 1 {
        let height = height.min(space);
        return PaletteLayout {
          area: Rect::new(left, self.composer.y.saturating_sub(height), width, height),
          rows,
          has_more,
        };
      }
      rows -= 1;
    }
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
