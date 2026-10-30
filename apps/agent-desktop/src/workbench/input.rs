use super::state::{Reply, Workbench};
use agent_protocol::v1::BrowserInput;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{TestSupportExt as _, *};

impl Workbench {
  pub(super) fn browser_input(&mut self, mut input: BrowserInput, cx: &mut Context<Self>) {
    if self.busy
      || (input.kind != "resize" && !self.space.as_ref().is_some_and(|space| space.human_control))
    {
      return;
    }
    let Some((frame, _)) = self.frame.as_ref() else {
      return;
    };
    if input.kind != "resize"
      && self
        .requested_viewport
        .is_some_and(|size| size != (frame.width, frame.height))
    {
      return;
    }
    input.tab_id = frame.tab_id.clone();
    input.revision = frame.revision;
    if self.input_queue.len() >= 128 {
      self.browser_error = Some("Browser input queue is full; return control and retry".into());
      return;
    }
    if input.kind == "mouse_move"
      && self
        .input_queue
        .back()
        .is_some_and(|event| event.kind == "mouse_move")
    {
      self.input_queue.pop_back();
    }
    if input.kind == "text"
      && let Some(last) = self.input_queue.back_mut()
      && last.kind == "text"
      && last.text.len() + input.text.len() <= 65536
    {
      last.text.push_str(&input.text);
      cx.notify();
      return;
    }
    self.input_queue.push_back(input);
    self.send_next_input();
    cx.notify();
  }
  pub(super) fn send_next_input(&mut self) {
    if self.input_pending || self.input_frame_pending || self.browser_error.is_some() {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    let Some((frame, _)) = &self.frame else {
      return;
    };
    let Some(mut input) = self.input_queue.pop_front() else {
      return;
    };
    if input.tab_id != frame.tab_id {
      self.input_queue.clear();
      self.browser_error =
        Some("Browser page changed before input was sent; refresh the view and retry.".into());
      return;
    }
    // Bind unsent queued events to the acknowledged current view. A dispatched
    // failure clears the queue and is never retried.
    input.revision = frame.revision;
    self.inputs_sent = self.inputs_sent.saturating_add(1);
    if input.kind.starts_with("mouse") {
      self.last_pointer = Some((input.x, input.y));
    }
    self.input_pending = true;
    self.request(async move {
      Ok(match client.browser_input(input).await {
        Ok(()) => Reply::Input,
        Err(error) => Reply::InputError(error.to_string()),
      })
    });
  }
  pub(super) fn resize_browser(&mut self, cx: &mut Context<Self>) {
    let bounds = self.browser_bounds.get();
    let width = f32::from(bounds.size.width).round().max(1.) as u32;
    let height = f32::from(bounds.size.height).round().max(1.) as u32;
    if width < 16
      || height < 16
      || self.frame.is_none()
      || self.input_pending
      || self.browser_error.is_some()
      || u64::from(width) * u64::from(height) > 16 * 1024 * 1024 / 4
    {
      return;
    }
    if self
      .frame
      .as_ref()
      .is_some_and(|(frame, _)| frame.width == width && frame.height == height)
    {
      self.requested_viewport = Some((width, height));
      return;
    }
    if self.requested_viewport != Some((width, height)) {
      self.requested_viewport = Some((width, height));
      self.browser_input(
        BrowserInput {
          kind: "resize".into(),
          width,
          height,
          ..Default::default()
        },
        cx,
      );
    }
  }
  fn browser_key(&mut self, key: &Keystroke, pressed: bool, cx: &mut Context<Self>) {
    let mut code = match key.key.as_str() {
      "enter" => 13,
      "tab" => 9,
      "backspace" => 8,
      "escape" => 27,
      "left" => 37,
      "up" => 38,
      "right" => 39,
      "down" => 40,
      "delete" => 46,
      _ => 0,
    };
    if code == 0
      && (key.modifiers.control || key.modifiers.platform || key.modifiers.alt)
      && key.key.len() == 1
    {
      code = i32::from(key.key.as_bytes()[0].to_ascii_uppercase());
    }
    let modifiers = u32::from(key.modifiers.alt)
      | (u32::from(key.modifiers.control) * 2)
      | (u32::from(key.modifiers.platform) * 4)
      | (u32::from(key.modifiers.shift) * 8);
    if code != 0 {
      self.browser_input(
        BrowserInput {
          kind: if pressed { "key_down" } else { "key_up" }.into(),
          key_code: code,
          modifiers,
          ..Default::default()
        },
        cx,
      );
    }
  }
  fn pointer(&self, point: Point<Pixels>) -> (i32, i32) {
    let bounds = self.browser_bounds.get();
    let (width, height) = self
      .frame
      .as_ref()
      .map_or((1, 1), |(frame, _)| (frame.width, frame.height));
    let x = (f32::from(point.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.)
      * width as f32) as i32;
    let y = (f32::from(point.y - bounds.origin.y) / f32::from(bounds.size.height).max(1.)
      * height as f32) as i32;
    (
      x.clamp(0, width.saturating_sub(1) as i32),
      y.clamp(0, height.saturating_sub(1) as i32),
    )
  }
  pub(super) fn browser_surface(&self, cx: &mut Context<Self>) -> AnyElement {
    let bounds = self.browser_bounds.clone();
    let view = cx.entity();
    let focus = self.browser_focus.clone();
    if let Some((_, image)) = &self.frame {
      div()
        .id("browser-surface")
        .test_support()
        .size_full()
        .relative()
        .track_focus(&self.browser_focus)
        .child(img(image.clone()).size_full())
        .child(
          canvas(
            move |rect, _, _| {
              bounds.set(rect);
            },
            move |rect, _, window, cx| {
              window.handle_input(&focus, ElementInputHandler::new(rect, view), cx);
            },
          )
          .absolute()
          .top_0()
          .left_0()
          .size_full(),
        )
        .on_mouse_down(
          MouseButton::Left,
          cx.listener(|this, event: &MouseDownEvent, window, cx| {
            this.browser_focus.focus(window, cx);
            let (x, y) = this.pointer(event.position);
            this.browser_input(
              BrowserInput {
                kind: "mouse_down".into(),
                x,
                y,
                button: 0,
                ..Default::default()
              },
              cx,
            );
          }),
        )
        .on_mouse_up(
          MouseButton::Left,
          cx.listener(|this, event: &MouseUpEvent, _, cx| {
            let (x, y) = this.pointer(event.position);
            this.browser_input(
              BrowserInput {
                kind: "mouse_up".into(),
                x,
                y,
                button: 0,
                ..Default::default()
              },
              cx,
            );
          }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
          let (x, y) = this.pointer(event.position);
          this.browser_input(
            BrowserInput {
              kind: "mouse_move".into(),
              x,
              y,
              ..Default::default()
            },
            cx,
          );
        }))
        .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
          let (x, y) = this.pointer(event.position);
          let delta = event.delta.pixel_delta(px(20.));
          this.browser_input(
            BrowserInput {
              kind: "wheel".into(),
              x,
              y,
              delta_x: f32::from(delta.x) as i32,
              delta_y: f32::from(delta.y) as i32,
              ..Default::default()
            },
            cx,
          );
        }))
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
          this.browser_key(&event.keystroke, true, cx)
        }))
        .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
          this.browser_key(&event.keystroke, false, cx)
        }))
        .into_any_element()
    } else {
      div().p_6().text_sm().text_color(cx.theme().muted_foreground)
      .child("Open a page to connect the CEF runtime. Browser frames are received from the authenticated daemon.").into_any_element()
    }
  }
}
