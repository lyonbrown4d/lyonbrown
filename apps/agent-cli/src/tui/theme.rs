use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
  pub id: &'static str,
  pub display_name: &'static str,
  pub background: Color,
  pub surface_raised: Color,
  pub surface_active: Color,
  pub text_primary: Color,
  pub text_secondary: Color,
  pub text_faint: Color,
  pub accent_primary: Color,
  pub accent_secondary: Color,
  pub status_success: Color,
  pub status_warning: Color,
  pub status_danger: Color,
  pub border_default: Color,
  pub focus_ring: Color,
}

impl Theme {
  pub fn built_ins() -> &'static [Self] {
    &BUILT_INS
  }

  pub fn from_id(id: &str) -> Self {
    BUILT_INS
      .iter()
      .copied()
      .find(|theme| theme.id == id)
      .unwrap_or_default()
  }

  pub fn rgb_tokens(&self) -> [u32; 12] {
    [
      as_rgb(self.background),
      as_rgb(self.surface_raised),
      as_rgb(self.surface_active),
      as_rgb(self.text_primary),
      as_rgb(self.text_secondary),
      as_rgb(self.text_faint),
      as_rgb(self.accent_primary),
      as_rgb(self.accent_secondary),
      as_rgb(self.status_success),
      as_rgb(self.status_warning),
      as_rgb(self.status_danger),
      as_rgb(self.border_default),
    ]
  }
}

impl Default for Theme {
  fn default() -> Self {
    BUILT_INS[3]
  }
}

const fn color(value: u32) -> Color {
  Color::Rgb(
    ((value >> 16) & 0xff) as u8,
    ((value >> 8) & 0xff) as u8,
    (value & 0xff) as u8,
  )
}

fn as_rgb(color: Color) -> u32 {
  match color {
    Color::Rgb(red, green, blue) => {
      (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
    }
    _ => 0,
  }
}

macro_rules! theme {
  ($id:literal, $name:literal, [$($token:literal),+ $(,)?]) => {{
    let [background, raised, active, text, secondary_text, faint_text, accent, secondary_accent,
      success, warning, danger, border] = [$(color($token)),+];
    Theme {
      id: $id,
      display_name: $name,
      background,
      surface_raised: raised,
      surface_active: active,
      text_primary: text,
      text_secondary: secondary_text,
      text_faint: faint_text,
      accent_primary: accent,
      accent_secondary: secondary_accent,
      status_success: success,
      status_warning: warning,
      status_danger: danger,
      border_default: border,
      focus_ring: accent,
    }
  }};
}

const BUILT_INS: [Theme; 6] = [
  theme!(
    "carbon-mint",
    "碳墨薄荷",
    [
      0x090c10, 0x121820, 0x18212b, 0xe8edf2, 0x9aa5b1, 0x647080, 0x5eead4, 0xa5b4fc, 0x4ade80,
      0xfbbf24, 0xfb7185, 0x273240,
    ]
  ),
  theme!(
    "ink-violet",
    "墨夜紫罗兰",
    [
      0x0c0b10, 0x16141d, 0x1d1928, 0xf0edf5, 0xa39cac, 0x6f687a, 0xa78bfa, 0x7dd3fc, 0x5eead4,
      0xf6c453, 0xfb7185, 0x332d40,
    ]
  ),
  theme!(
    "slate-blue",
    "岩板蓝",
    [
      0x0a0f16, 0x111a24, 0x182433, 0xe6edf5, 0x98a6b8, 0x64748b, 0x60a5fa, 0x67e8f9, 0x34d399,
      0xf5b942, 0xf87171, 0x2b3a4d,
    ]
  ),
  theme!(
    "hearth-teal",
    "炉火深青",
    [
      0x0b1113, 0x122024, 0x182a2f, 0xeae7df, 0xa8a39a, 0x716d66, 0xe07a5f, 0x74c7b8, 0x6fcf97,
      0xe9b949, 0xe76f76, 0x2a3a3d,
    ]
  ),
  theme!(
    "sandstone-espresso",
    "砂岩咖啡",
    [
      0x100e0c, 0x1a1713, 0x241f19, 0xeee9e0, 0xa9a093, 0x70685e, 0xd4a373, 0x8fb996, 0x7bc8a4,
      0xe8b44f, 0xe27d75, 0x3a3229,
    ]
  ),
  theme!(
    "graphite-coral",
    "石墨珊瑚",
    [
      0x101114, 0x191a1f, 0x22242b, 0xeceef2, 0x9da3ae, 0x666c78, 0xf28c70, 0x82a7c9, 0x74c69d,
      0xf0b95b, 0xef6f78, 0x30333d,
    ]
  ),
];
