use crate::{Action, BrowserError, BrowserResult};

pub fn validate_url(input: &str) -> BrowserResult<url::Url> {
  let url =
    url::Url::parse(input).map_err(|_| BrowserError::InvalidInput("invalid browser URL".into()))?;
  if !matches!(url.scheme(), "http" | "https" | "about")
    || (url.scheme() == "about" && input != "about:blank")
    || !url.username().is_empty()
    || url.password().is_some()
  {
    return Err(BrowserError::InvalidInput(
      "only credential-free HTTP(S) and about:blank URLs are supported".into(),
    ));
  }
  // Local addresses remain intentional browser targets, subject to agentd's permission policy.
  Ok(url)
}

pub fn validate_action(action: &Action) -> BrowserResult<()> {
  if action.id.is_empty()
    || action.snapshot_id.is_empty()
    || action.tab_id.is_empty()
    || action.text.len() > 64 * 1024
    || !(1..=30_000).contains(&action.timeout_ms)
  {
    return Err(BrowserError::InvalidInput(
      "invalid action identity, text size or timeout".into(),
    ));
  }
  if !matches!(
    action.kind.as_str(),
    "click" | "fill" | "press" | "navigate"
  ) {
    return Err(BrowserError::InvalidInput(
      "unsupported semantic browser action".into(),
    ));
  }
  if action.kind == "navigate" {
    validate_url(&action.text)?;
  }
  Ok(())
}

pub(crate) fn safe_profile_id(id: &str) -> BrowserResult<()> {
  if id.is_empty()
    || id.len() > 96
    || !id
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
  {
    return Err(BrowserError::InvalidInput(
      "profile id must contain 1–96 ASCII letters, digits, underscores or hyphens".into(),
    ));
  }
  Ok(())
}
