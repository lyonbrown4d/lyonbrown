use crate::service::Mapping;

pub(crate) fn inside_placeholder(
  text: &str,
  start: usize,
  end: usize,
  mappings: Option<&[Mapping]>,
) -> bool {
  let Some(prefix) = text[..start].rfind("[PRIVATE:") else {
    return false;
  };
  let Some(candidate) = text.get(prefix..prefix.saturating_add(42)) else {
    return false;
  };
  end <= prefix + candidate.len()
    && mappings.is_some_and(|items| items.iter().any(|item| item.placeholder == candidate))
    && candidate
      .strip_prefix("[PRIVATE:")
      .and_then(|s| s.strip_suffix(']'))
      .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub(crate) fn is_placeholder(text: &str) -> bool {
  text
    .strip_prefix("[PRIVATE:")
    .and_then(|s| s.strip_suffix(']'))
    .is_some_and(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}
