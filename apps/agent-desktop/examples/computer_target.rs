//! Isolated, synthetic target for observing native click and text effects.
use gpui_kit::component::{
  ActiveTheme, TitleBar,
  button::{Button, ButtonVariants},
  input::{Input, InputEvent, InputState},
};
use gpui_kit::*;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

const TITLE: &str = "Lyonbrown Computer Target · isolated fixture";

struct ComputerTarget {
  directory: PathBuf,
  clicks: u64,
  input: Entity<InputState>,
  error: Option<String>,
  _input_subscription: Subscription,
}

impl ComputerTarget {
  fn new(directory: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Native test text"));
    let subscription = cx.subscribe_in(&input, window, |this, _, event, _, cx| {
      if matches!(event, InputEvent::Change) {
        this.persist(cx);
        cx.notify();
      }
    });
    Self {
      directory,
      clicks: 0,
      input,
      error: None,
      _input_subscription: subscription,
    }
  }

  fn persist(&mut self, cx: &App) {
    let result = write_state(
      &self.directory,
      self.clicks,
      self.input.read(cx).value().as_ref(),
    );
    self.error = result.err().map(|error| {
      let message = format!("Could not persist target state: {error:#}");
      eprintln!("{message}");
      message
    });
  }
}

impl Render for ComputerTarget {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    div()
      .size_full()
      .flex()
      .flex_col()
      .bg(cx.theme().background)
      .text_color(cx.theme().foreground)
      .child(TitleBar::new().child(TITLE))
      .child(
        div()
          .flex_1()
          .p_6()
          .flex()
          .flex_col()
          .gap_4()
          .child("Synthetic native Computer acceptance target")
          .child(div().text_sm().child(
            "Use only this isolated window. Click the button and enter the synthetic marker.",
          ))
          .child(
            Button::new("verify-native-click")
              .primary()
              .label("Verify native click")
              .on_click(cx.listener(|this, _, _, cx| {
                this.clicks = this.clicks.saturating_add(1);
                this.persist(cx);
                cx.notify();
              })),
          )
          .child(format!("Click count: {}", self.clicks))
          .child("Native test text")
          .child(Input::new(&self.input).id("native-test-text"))
          .child(format!("Current text: {}", self.input.read(cx).value()))
          .children(self.error.clone().map(|error| div().text_sm().child(error))),
      )
  }
}

fn write_state(directory: &std::path::Path, clicks: u64, text: &str) -> anyhow::Result<()> {
  let pending = directory.join("state.json.pending");
  std::fs::write(
    &pending,
    serde_json::to_vec_pretty(&serde_json::json!({ "click_count": clicks, "text": text }))?,
  )?;
  std::fs::rename(pending, directory.join("state.json"))?;
  Ok(())
}

fn main() -> anyhow::Result<()> {
  let acceptance = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/acceptance");
  std::fs::create_dir_all(&acceptance)?;
  let directory = tempfile::Builder::new()
    .prefix("computer-target-")
    .tempdir_in(acceptance)?
    .keep();
  write_state(&directory, 0, "")?;
  println!("{}", directory.display());
  let failure = Rc::new(RefCell::new(None));
  let window_failure = failure.clone();
  gpui_kit::application()
    .with_assets(gpui_kit::assets::Assets)
    .run(move |cx| {
      gpui_kit::init(cx);
      cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
          cx.quit();
        }
      })
      .detach();
      if let Err(error) = gpui_kit::open_window(
        WindowOptions {
          window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(640.), px(400.)),
            cx,
          ))),
          ..TitleBar::window_options()
        },
        cx,
        move |window, cx| {
          window.set_window_title(TITLE);
          window.activate_window();
          cx.new(|cx| ComputerTarget::new(directory, window, cx))
        },
      ) {
        *window_failure.borrow_mut() = Some(error);
        cx.quit();
      }
      cx.activate(true);
    });
  if let Some(error) = failure.take() {
    return Err(error);
  }
  Ok(())
}
