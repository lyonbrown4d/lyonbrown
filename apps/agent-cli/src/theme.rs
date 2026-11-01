use ratatui::style::Color;

use crate::config::ThemeChoice;

#[derive(Clone, Copy)]
pub struct Theme {
  pub background: Color,
  pub text: Color,
  pub muted: Color,
  pub accent: Color,
  pub warning: Color,
  pub diff: DiffColors,
  pub syntax: SyntaxColors,
}

/// Diff tints stay close to the surface colour so long hunks remain readable.
#[derive(Clone, Copy)]
pub struct DiffColors {
  pub added: Color,
  pub removed: Color,
  pub added_bg: Color,
  pub removed_bg: Color,
  pub hunk: Color,
}

/// tree-sitter capture colours. Unknown captures fall back to `Theme::text`.
#[derive(Clone, Copy)]
pub struct SyntaxColors {
  pub attribute: Color,
  pub comment: Color,
  pub constant: Color,
  pub function: Color,
  pub keyword: Color,
  pub number: Color,
  pub operator: Color,
  pub property: Color,
  pub punctuation: Color,
  pub string: Color,
  pub type_name: Color,
  pub variable: Color,
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
      diff: if light {
        DiffColors {
          added: Color::Rgb(45, 105, 55),
          removed: Color::Rgb(150, 55, 45),
          added_bg: Color::Rgb(224, 240, 226),
          removed_bg: Color::Rgb(250, 228, 225),
          hunk: Color::Rgb(90, 105, 130),
        }
      } else {
        DiffColors {
          added: Color::Rgb(140, 200, 140),
          removed: Color::Rgb(228, 138, 128),
          added_bg: Color::Rgb(28, 46, 32),
          removed_bg: Color::Rgb(52, 28, 28),
          hunk: Color::Rgb(126, 148, 175),
        }
      },
      syntax: if light {
        SyntaxColors {
          attribute: Color::Rgb(130, 88, 36),
          comment: Color::Rgb(139, 134, 124),
          constant: Color::Rgb(110, 60, 120),
          function: Color::Rgb(118, 88, 18),
          keyword: Color::Rgb(150, 70, 40),
          number: Color::Rgb(110, 60, 120),
          operator: Color::Rgb(88, 86, 78),
          property: Color::Rgb(30, 110, 100),
          punctuation: Color::Rgb(120, 118, 110),
          string: Color::Rgb(70, 108, 50),
          type_name: Color::Rgb(40, 90, 130),
          variable: Color::Rgb(51, 55, 49),
        }
      } else {
        SyntaxColors {
          attribute: Color::Rgb(205, 175, 130),
          comment: Color::Rgb(122, 120, 112),
          constant: Color::Rgb(198, 158, 198),
          function: Color::Rgb(226, 190, 126),
          keyword: Color::Rgb(228, 150, 112),
          number: Color::Rgb(198, 158, 198),
          operator: Color::Rgb(180, 175, 165),
          property: Color::Rgb(150, 200, 186),
          punctuation: Color::Rgb(150, 148, 140),
          string: Color::Rgb(152, 190, 124),
          type_name: Color::Rgb(148, 178, 206),
          variable: Color::Rgb(239, 235, 227),
        }
      },
    }
  }
}
