#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UiLocale {
  #[default]
  EnUs,
  ZhCn,
}

impl UiLocale {
  pub fn toggled(self) -> Self {
    match self {
      Self::EnUs => Self::ZhCn,
      Self::ZhCn => Self::EnUs,
    }
  }

  pub fn switch_label(self) -> &'static str {
    match self {
      Self::EnUs => "中文",
      Self::ZhCn => "EN",
    }
  }
}

#[derive(Clone, Copy)]
pub(crate) struct Copy {
  locale: UiLocale,
}

impl Copy {
  pub fn new(locale: UiLocale) -> Self {
    Self { locale }
  }

  pub fn text(self, english: &'static str, chinese: &'static str) -> &'static str {
    match self.locale {
      UiLocale::EnUs => english,
      UiLocale::ZhCn => chinese,
    }
  }
}

pub fn composer_placeholder(locale: UiLocale) -> &'static str {
  Copy::new(locale).text("Describe the next task…", "描述下一项任务…")
}

pub fn command_error_text(error: &str, locale: UiLocale) -> String {
  if error.trim().is_empty() {
    Copy::new(locale)
      .text("Runtime command failed", "运行时命令失败")
      .to_owned()
  } else {
    error.to_owned()
  }
}
