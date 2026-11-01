use percent_encoding::percent_decode_str;
use pulldown_cmark::{Event, Parser, Tag};

use crate::{Result, SkillError, paths};

/// Validate file links as data. Remote links remain inert and are not fetched.
pub(crate) fn validate(content: &str, origin: &str, max_depth: usize) -> Result<()> {
  for event in Parser::new(content) {
    let destination = match event {
      Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => dest_url,
      _ => continue,
    };
    let destination = destination.split(['#', '?']).next().unwrap_or_default();
    if destination.is_empty() {
      continue;
    }
    if let Ok(url) = url::Url::parse(destination) {
      if !matches!(url.scheme(), "https" | "http" | "mailto") {
        return Err(SkillError::UnsafePath);
      }
      continue;
    }
    let decoded = percent_decode_str(destination)
      .decode_utf8()
      .map_err(|_| SkillError::UnsafePath)?;
    if decoded.starts_with('/') || decoded.contains(['\\', ':']) {
      return Err(SkillError::UnsafePath);
    }
    let mut parts: Vec<_> = origin.split('/').collect();
    parts.pop();
    for part in decoded.split('/') {
      match part {
        "." | "" => {}
        ".." => {
          if parts.pop().is_none() {
            return Err(SkillError::UnsafePath);
          }
        }
        part => parts.push(part),
      }
    }
    paths::relative(&parts.join("/"), max_depth)?;
  }
  Ok(())
}
