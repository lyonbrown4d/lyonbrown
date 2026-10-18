use super::state::{Reply, Workbench};
use agent_protocol::v1::BrowserFrame;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, button::Button, input::Input};
use gpui_kit::{TestSupportExt as _, *};
use std::sync::Arc;

impl Workbench {
  fn request_browser<F>(&mut self, future: F)
  where
    F: std::future::Future<Output = anyhow::Result<Reply>> + Send + 'static,
  {
    self.pending_browser_operation = true;
    self.request(async move {
      Ok(
        future
          .await
          .unwrap_or_else(|error| Reply::BrowserOperationError(error.to_string())),
      )
    })
  }

  pub(super) fn open_browser(&mut self, cx: &mut Context<Self>) {
    if self.pending_browser_operation || self.connection_pending {
      return;
    }
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    if self
      .attachment
      .as_ref()
      .is_some_and(|attachment| attachment.is_finished())
    {
      self.browser_sessions.clear();
      self.attachment = None;
      self.space = None;
      self.tab = None;
      self.frame = None;
    }
    if self.attachment.is_none() {
      let browser = self.browser.clone();
      self.busy = true;
      self.browser_error = None;
      self.request_browser(async move { Ok(Reply::Attached(browser.attach(client).await?)) });
      cx.notify();
      return;
    }
    let host_id = self
      .attachment
      .as_ref()
      .map(|attachment| attachment.host_id.clone())
      .unwrap_or_default();
    let url = self.url.read(cx).value().to_string();
    let space = self.space.clone();
    self.input_queue.clear();
    self.busy = true;
    self.browser_error = None;
    self.request_browser(async move {
      let space = match space {
        Some(space) => space,
        None => {
          client
            .open_browser_space_on_host(&session, &session, false, &host_id)
            .await?
        }
      };
      Ok(match client.open_browser_tab(&space.id, &url).await {
        Ok(tab) => Reply::Tab(space, tab),
        // Opening a Space already claimed its profile. Keep that identity even
        // when navigation fails so a deliberate retry uses the same owner.
        Err(error) => Reply::TabOpenFailed(space, error.to_string()),
      })
    });
    cx.notify();
  }
  fn close_browser(&mut self, cx: &mut Context<Self>) {
    if self.pending_browser_operation || self.connection_pending {
      return;
    }
    let (Some(client), Some(space)) = (self.client.clone(), self.space.clone()) else {
      return;
    };
    self.busy = true;
    self.request_browser(async move {
      client.close_browser_space(&space.id).await?;
      Ok(Reply::Closed)
    });
    cx.notify();
  }
  fn takeover(&mut self, cx: &mut Context<Self>) {
    if self.pending_browser_operation || self.connection_pending {
      return;
    }
    let (Some(client), Some(space)) = (self.client.clone(), self.space.clone()) else {
      return;
    };
    self.busy = true;
    self.browser_error = None;
    self.request_browser(async move {
      Ok(Reply::Control(
        client
          .browser_takeover(&space.id, !space.human_control)
          .await?,
      ))
    });
    cx.notify();
  }
  pub(super) fn accept_frame(&mut self, frame: BrowserFrame) {
    if frame.bgra8.is_empty() {
      return;
    }
    let length = frame.width.checked_mul(4).and_then(|stride| {
      if stride == frame.stride {
        stride.checked_mul(frame.height)
      } else {
        None
      }
    });
    if frame.width == 0
      || frame.height == 0
      || length
        .is_none_or(|length| length as usize != frame.bgra8.len() || length > 16 * 1024 * 1024)
    {
      self.browser_error = Some("Browser returned an invalid or oversized native frame".into());
      return;
    }
    // GPUI's RenderImage upload uses BGRA, matching the owned CEF OSR frame.
    if let Some(pixels) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra8.clone())
    {
      let image = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
      self.frame = Some((frame, image));
    }
  }
  pub(super) fn browser_view(&self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
    let human = self.space.as_ref().is_some_and(|space| space.human_control);
    let surface = self.browser_surface(cx);
    div()
      .id("browser-panel")
      .test_support()
      .flex()
      .flex_col()
      .size_full()
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .p_2()
          .child(
            div()
              .flex_1()
              .min_w_0()
              .child(Input::new(&self.url).id("browser-url").small()),
          )
          .child(
            Button::new("browser-open")
              .small()
              .label(
                if self
                  .attachment
                  .as_ref()
                  .is_some_and(|attachment| attachment.is_finished())
                {
                  "Restart runtime"
                } else {
                  "Open"
                },
              )
              .disabled(
                self.busy
                  || self.pending_browser_operation
                  || self.connection_pending
                  || self.session.is_none(),
              )
              .on_click(cx.listener(|this, _, _, cx| this.open_browser(cx))),
          ),
      )
      .child(
        div()
          .flex()
          .items_center()
          .gap_2()
          .px_2()
          .child(
            Button::new("browser-takeover")
              .small()
              .label(if human {
                "Return to agent"
              } else {
                "Take control"
              })
              .disabled(
                self.space.is_none()
                  || self.busy
                  || self.pending_browser_operation
                  || self.connection_pending,
              )
              .on_click(cx.listener(|this, _, _, cx| this.takeover(cx))),
          )
          .child(
            Button::new("browser-refresh")
              .small()
              .label("Refresh view")
              .disabled(self.tab.is_none() || self.busy)
              .on_click(cx.listener(|this, _, _, cx| {
                this.browser_error = None;
                this.requested_viewport = None;
                this.fetch_frame(cx);
                cx.notify();
              })),
          )
          .child(
            Button::new("browser-close")
              .small()
              .label("Close")
              .disabled(
                self.space.is_none()
                  || self.busy
                  || self.pending_browser_operation
                  || self.connection_pending,
              )
              .on_click(cx.listener(|this, _, _, cx| this.close_browser(cx))),
          )
          .child(div().text_xs().child(if human {
            "Human control"
          } else {
            "Agent control · view only"
          })),
      )
      .child(
        div()
          .id("browser-error")
          .test_support()
          .p_2()
          .text_xs()
          .text_color(cx.theme().danger)
          .child(self.browser_error.clone().unwrap_or_default()),
      )
      .child(
        div()
          .text_xs()
          .px_2()
          .child(self.marked.clone().unwrap_or_default()),
      )
      .child(div().flex_1().min_h_0().child(surface))
      .into_any_element()
  }
}
