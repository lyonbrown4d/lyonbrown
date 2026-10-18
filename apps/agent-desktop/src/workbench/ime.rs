use super::state::Workbench;
use agent_protocol::v1::BrowserInput;
use gpui_kit::*;
use std::ops::Range;

/// Native text/IME composition stays in the UI until committed to Chromium.
/// The daemon receives committed text through the same authorized input channel.
impl EntityInputHandler for Workbench {
  fn text_for_range(
    &mut self,
    range: Range<usize>,
    adjusted: &mut Option<Range<usize>>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<String> {
    let text = self.marked.as_deref().unwrap_or_default();
    let utf16: Vec<_> = text.encode_utf16().collect();
    let end = range.end.min(utf16.len());
    let start = range.start.min(end);
    *adjusted = Some(start..end);
    String::from_utf16(&utf16[start..end]).ok()
  }
  fn selected_text_range(
    &mut self,
    _: bool,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<UTF16Selection> {
    let end = self
      .marked
      .as_deref()
      .unwrap_or_default()
      .encode_utf16()
      .count();
    Some(UTF16Selection {
      range: end..end,
      reversed: false,
    })
  }
  fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
    self
      .marked
      .as_ref()
      .map(|text| 0..text.encode_utf16().count())
  }
  fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
    self.marked = None;
    cx.notify();
  }
  fn replace_text_in_range(
    &mut self,
    _: Option<Range<usize>>,
    text: &str,
    _: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self.marked = None;
    self.browser_input(
      BrowserInput {
        kind: "text".into(),
        text: text.into(),
        ..Default::default()
      },
      cx,
    );
  }
  fn replace_and_mark_text_in_range(
    &mut self,
    _: Option<Range<usize>>,
    text: &str,
    _: Option<Range<usize>>,
    _: &mut Window,
    cx: &mut Context<Self>,
  ) {
    if text.len() <= 65536 {
      self.marked = Some(text.to_string());
      cx.notify();
    }
  }
  fn bounds_for_range(
    &mut self,
    _: Range<usize>,
    _: Bounds<Pixels>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<Bounds<Pixels>> {
    let bounds = self.browser_bounds.get();
    Some(Bounds {
      origin: bounds.origin,
      size: size(px(1.), px(20.)),
    })
  }
  fn character_index_for_point(
    &mut self,
    _: Point<Pixels>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<usize> {
    Some(0)
  }
  fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
    !self.busy
      && self.frame.is_some()
      && self.space.as_ref().is_some_and(|space| space.human_control)
  }
}
