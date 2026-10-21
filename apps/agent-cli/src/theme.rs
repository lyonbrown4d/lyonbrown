use ratatui::style::Color;

use crate::config::ThemeChoice;

#[derive(Clone, Copy)]
pub struct Theme {
  pub background: Color,
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
    Self {
      background: if light {
        Color::Rgb(245, 242, 233)
      } else {
        Color::Rgb(24, 24, 23)
      },
      text: if light {
        Color::Rgb(51, 55, 49)
      } else {
        Color::Rgb(239, 235, 227)
      },
      muted: if light {
        Color::Rgb(100, 99, 92)
      } else {
        Color::Rgb(167, 165, 157)
      },
      accent: if light {
        Color::Rgb(143, 77, 44)
      } else {
        Color::Rgb(213, 154, 120)
      },
      warning: if light {
        Color::Rgb(157, 53, 40)
      } else {
        Color::Rgb(237, 177, 106)
      },
    }
  }
}
