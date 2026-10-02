mod element;

use std::ops::Range;

use gpui::{
  App, Bounds, Context, EntityInputHandler, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
  MouseButton, Pixels, ShapedLine, UTF16Selection, Window, div, prelude::*, px,
};

use element::ComposerTextElement;

const MAX_PROMPT_CHARS: usize = 16_384;

pub(crate) struct SubmitRequested;

pub(crate) struct ComposerInput {
  focus: FocusHandle,
  content: String,
  selected: Range<usize>,
  marked: Option<Range<usize>>,
  last_layout: Option<ShapedLine>,
  last_bounds: Option<Bounds<Pixels>>,
  enabled: bool,
}

impl ComposerInput {
  pub(crate) fn new(cx: &mut Context<Self>) -> Self {
    Self {
      focus: cx.focus_handle(),
      content: String::new(),
      selected: 0..0,
      marked: None,
      last_layout: None,
      last_bounds: None,
      enabled: false,
    }
  }

  pub(crate) fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
    if self.enabled != enabled {
      self.enabled = enabled;
      cx.notify();
    }
  }

  pub(crate) fn text(&self) -> &str {
    &self.content
  }

  pub(crate) fn take_text(&mut self, cx: &mut Context<Self>) -> String {
    let text = std::mem::take(&mut self.content);
    self.selected = 0..0;
    self.marked = None;
    cx.notify();
    text
  }

  fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
    match event.keystroke.key.as_str() {
      "enter" if self.enabled && self.marked.is_none() && !self.content.trim().is_empty() => {
        cx.emit(SubmitRequested)
      }
      "backspace" if self.marked.is_none() && self.selected.start > 0 => {
        let previous = self.content[..self.selected.start]
          .char_indices()
          .next_back()
          .map_or(0, |(index, _)| index);
        self.content.replace_range(previous..self.selected.end, "");
        self.selected = previous..previous;
        cx.notify();
      }
      _ => {}
    }
  }

  fn offset_from_utf16(&self, offset: usize) -> usize {
    self
      .content
      .char_indices()
      .map(|(index, character)| (index, character.len_utf16()))
      .scan(0, |count, (index, width)| {
        let current = *count;
        *count += width;
        Some((index, current))
      })
      .find_map(|(index, count)| (count >= offset).then_some(index))
      .unwrap_or(self.content.len())
  }

  fn offset_to_utf16(&self, offset: usize) -> usize {
    self.content[..offset.min(self.content.len())]
      .chars()
      .map(char::len_utf16)
      .sum()
  }

  fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
    self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
  }

  fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
    self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
  }

  fn replace(&mut self, range_utf16: Option<Range<usize>>, new_text: &str) {
    if !self.enabled {
      return;
    }
    let range = range_utf16
      .as_ref()
      .map(|range| self.range_from_utf16(range))
      .or(self.marked.clone())
      .unwrap_or_else(|| self.selected.clone());
    let available = MAX_PROMPT_CHARS.saturating_sub(
      self
        .content
        .chars()
        .count()
        .saturating_sub(self.content[range.clone()].chars().count()),
    );
    let replacement: String = new_text
      .chars()
      .map(|character| {
        if matches!(character, '\r' | '\n') {
          ' '
        } else {
          character
        }
      })
      .take(available)
      .collect();
    self.content.replace_range(range.clone(), &replacement);
    let cursor = range.start + replacement.len();
    self.selected = cursor..cursor;
    self.marked = None;
  }
}

impl EventEmitter<SubmitRequested> for ComposerInput {}

impl Focusable for ComposerInput {
  fn focus_handle(&self, _: &App) -> FocusHandle {
    self.focus.clone()
  }
}

impl EntityInputHandler for ComposerInput {
  fn text_for_range(
    &mut self,
    range: Range<usize>,
    actual: &mut Option<Range<usize>>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<String> {
    let range = self.range_from_utf16(&range);
    actual.replace(self.range_to_utf16(&range));
    self.content.get(range).map(ToOwned::to_owned)
  }

  fn selected_text_range(
    &mut self,
    _: bool,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<UTF16Selection> {
    Some(UTF16Selection {
      range: self.range_to_utf16(&self.selected),
      reversed: false,
    })
  }

  fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
    self.marked.as_ref().map(|range| self.range_to_utf16(range))
  }

  fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
    self.marked = None;
  }

  fn replace_text_in_range(
    &mut self,
    range: Option<Range<usize>>,
    text: &str,
    _: &mut Window,
    cx: &mut Context<Self>,
  ) {
    self.replace(range, text);
    cx.notify();
  }

  fn replace_and_mark_text_in_range(
    &mut self,
    range: Option<Range<usize>>,
    text: &str,
    selected: Option<Range<usize>>,
    _: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let start = range
      .as_ref()
      .map(|range| self.range_from_utf16(range).start)
      .or_else(|| self.marked.as_ref().map(|range| range.start))
      .unwrap_or(self.selected.start);
    self.replace(range, text);
    if !text.is_empty() {
      let end = self.selected.end;
      self.marked = Some(start..end);
      if let Some(selected) = selected {
        let selected_start = utf16_offset_in(text, selected.start);
        let selected_end = utf16_offset_in(text, selected.end);
        self.selected = start + selected_start..start + selected_end;
      }
    }
    cx.notify();
  }

  fn bounds_for_range(
    &mut self,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<Bounds<Pixels>> {
    let line = self.last_layout.as_ref()?;
    let range = self.range_from_utf16(&range);
    Some(Bounds::from_corners(
      gpui::point(bounds.left() + line.x_for_index(range.start), bounds.top()),
      gpui::point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
    ))
  }

  fn character_index_for_point(
    &mut self,
    point: gpui::Point<Pixels>,
    _: &mut Window,
    _: &mut Context<Self>,
  ) -> Option<usize> {
    let bounds = self.last_bounds?;
    let line = self.last_layout.as_ref()?;
    let index = line.index_for_x(point.x - bounds.left())?;
    Some(self.offset_to_utf16(index))
  }
}

fn utf16_offset_in(text: &str, offset: usize) -> usize {
  let mut utf16_count = 0;
  for (index, character) in text.char_indices() {
    if utf16_count >= offset {
      return index;
    }
    utf16_count += character.len_utf16();
  }
  text.len()
}

impl Render for ComposerInput {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let focus = self.focus.clone();
    div()
      .id("composer-input")
      .w_full()
      .track_focus(&self.focus)
      .on_key_down(cx.listener(Self::on_key_down))
      .on_mouse_down(MouseButton::Left, move |_, window, _| window.focus(&focus))
      .text_sm()
      .line_height(px(24.))
      .child(ComposerTextElement { input: cx.entity() })
  }
}
