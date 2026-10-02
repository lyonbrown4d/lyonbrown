use gpui::{
  App, Bounds, Element, ElementId, ElementInputHandler, Entity, GlobalElementId, IntoElement,
  LayoutId, Pixels, ShapedLine, SharedString, Style, TextRun, Window, relative,
};

use super::ComposerInput;
use crate::copy::composer_placeholder;

pub(super) struct ComposerTextElement {
  pub(super) input: Entity<ComposerInput>,
}

impl IntoElement for ComposerTextElement {
  type Element = Self;

  fn into_element(self) -> Self::Element {
    self
  }
}

impl Element for ComposerTextElement {
  type RequestLayoutState = ();
  type PrepaintState = ShapedLine;

  fn id(&self) -> Option<ElementId> {
    None
  }

  fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
    None
  }

  fn request_layout(
    &mut self,
    _: Option<&GlobalElementId>,
    _: Option<&gpui::InspectorElementId>,
    window: &mut Window,
    cx: &mut App,
  ) -> (LayoutId, ()) {
    let mut style = Style::default();
    style.size.width = relative(1.).into();
    style.size.height = window.line_height().into();
    (window.request_layout(style, [], cx), ())
  }

  fn prepaint(
    &mut self,
    _: Option<&GlobalElementId>,
    _: Option<&gpui::InspectorElementId>,
    _: Bounds<Pixels>,
    _: &mut (),
    window: &mut Window,
    cx: &mut App,
  ) -> ShapedLine {
    let input = self.input.read(cx);
    let text: SharedString = if input.content.is_empty() {
      composer_placeholder(input.locale).into()
    } else {
      input.content.clone().into()
    };
    let style = window.text_style();
    let run = TextRun {
      len: text.len(),
      font: style.font(),
      color: style.color,
      background_color: None,
      underline: None,
      strikethrough: None,
    };
    window.text_system().shape_line(
      text,
      style.font_size.to_pixels(window.rem_size()),
      &[run],
      None,
    )
  }

  fn paint(
    &mut self,
    _: Option<&GlobalElementId>,
    _: Option<&gpui::InspectorElementId>,
    bounds: Bounds<Pixels>,
    _: &mut (),
    line: &mut ShapedLine,
    window: &mut Window,
    cx: &mut App,
  ) {
    let focus = self.input.read(cx).focus.clone();
    window.handle_input(
      &focus,
      ElementInputHandler::new(bounds, self.input.clone()),
      cx,
    );
    line
      .paint(bounds.origin, window.line_height(), window, cx)
      .expect("composer text paint failed");
    self.input.update(cx, |input, _| {
      input.last_layout = Some(line.clone());
      input.last_bounds = Some(bounds);
    });
  }
}
