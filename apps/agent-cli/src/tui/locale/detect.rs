use super::Locale;

impl Locale {
  pub fn from_system_tag(tag: Option<&str>) -> Self {
    let Some(tag) = tag else { return Self::EnUs };
    let normalized = tag.replace('_', "-").to_ascii_lowercase();
    let language = normalized
      .split_once('.')
      .map_or(normalized.as_str(), |(value, _)| value);
    if language == "zh" || language.starts_with("zh-cn") || language.starts_with("zh-hans") {
      Self::ZhCn
    } else {
      Self::EnUs
    }
  }

  pub fn system_default() -> Self {
    Self::from_system_tag(sys_locale::get_locale().as_deref())
  }

  pub const fn toggled(self) -> Self {
    match self {
      Self::EnUs => Self::ZhCn,
      Self::ZhCn => Self::EnUs,
    }
  }
}
