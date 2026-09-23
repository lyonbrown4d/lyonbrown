use gpui::{Rgba, rgb};

pub const GRAPHITE: u32 = 0x111820;
pub const PANEL: u32 = 0x17232d;
pub const PAPER: u32 = 0xe7eef2;
pub const SIGNAL_CYAN: u32 = 0x38c6d4;
pub const APPROVAL_AMBER: u32 = 0xf1b84b;
pub const MUTED_STEEL: u32 = 0x78909c;
pub const HAIRLINE: u32 = 0x293842;
pub const DEEP_PANEL: u32 = 0x0d141b;

pub fn color(value: u32) -> Rgba {
  rgb(value)
}

#[cfg(target_os = "windows")]
pub const UI_FONT: &str = "Segoe UI";
#[cfg(target_os = "macos")]
pub const UI_FONT: &str = ".AppleSystemUIFont";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const UI_FONT: &str = "DejaVu Sans";

#[cfg(target_os = "windows")]
pub const DATA_FONT: &str = "Cascadia Mono";
#[cfg(target_os = "macos")]
pub const DATA_FONT: &str = "SF Mono";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const DATA_FONT: &str = "DejaVu Sans Mono";
