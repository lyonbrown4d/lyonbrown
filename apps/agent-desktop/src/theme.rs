//! Desktop surface tokens shared by native views; component behavior stays in gpui-kit.
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

pub(crate) const READING_WIDTH: f32 = 760.;
pub(crate) const SIDEBAR_WIDTH: f32 = 300.;
pub(crate) const RAIL_WIDTH: f32 = 48.;

#[derive(Clone, Copy)]
pub(crate) struct DesktopColors {
  pub canvas: Hsla,
  pub sidebar: Hsla,
  pub selection: Hsla,
  pub composer: Hsla,
  pub context: Hsla,
  pub prompt: Hsla,
  pub border: Hsla,
  pub foreground: Hsla,
  pub muted: Hsla,
}

impl DesktopColors {
  pub fn get(cx: &App) -> Self {
    let theme = cx.theme();
    if theme.mode.is_dark() {
      Self {
        canvas: rgb(0x1c1c1c).into(),
        sidebar: rgb(0x202123).into(),
        selection: rgb(0x34363a).into(),
        composer: rgb(0x262626).into(),
        context: rgb(0x242424).into(),
        prompt: rgb(0x303030).into(),
        border: rgb(0x414141).into(),
        foreground: rgb(0xe8e8e8).into(),
        muted: rgb(0xaaaaaa).into(),
      }
    } else {
      Self {
        canvas: rgb(0xffffff).into(),
        sidebar: rgb(0xeaf0f8).into(),
        selection: rgb(0xd8e2f0).into(),
        composer: rgb(0xffffff).into(),
        context: rgb(0xf4f4f5).into(),
        prompt: rgb(0xf0f1f3).into(),
        border: rgb(0xdcdde0).into(),
        foreground: rgb(0x202124).into(),
        muted: rgb(0x626872).into(),
      }
    }
  }
}
