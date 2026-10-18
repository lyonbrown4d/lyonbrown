use ratatui::style::Color;

use crate::config::ThemeChoice;

#[derive(Clone, Copy)]
pub struct Theme {
  pub text: Color,
  pub muted: Color,
  pub accent: Color,
  pub warning: Color,
}

impl Theme {
  pub fn resolve(choice: ThemeChoice, colorfgbg: Option<&str>) -> Self {
    let light = match choice {
      ThemeChoice::Light => true,
      ThemeChoice::Dark => false,
      ThemeChoice::Auto => colorfgbg
        .and_then(|value| value.rsplit(';').next())
        .and_then(|value| value.parse::<u8>().ok())
        .is_some_and(|background| (7..=15).contains(&background)),
    };
    // Reset preserves the terminal's actual background, including transparency.
    Self {
      text: Color::Reset,
      muted: if light { Color::DarkGray } else { Color::Gray },
      accent: if light { Color::Blue } else { Color::Cyan },
      warning: if light { Color::Red } else { Color::Yellow },
    }
  }
}
