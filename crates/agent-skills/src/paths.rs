use std::{collections::BTreeSet, path::PathBuf};

use crate::{Result, SkillError};

/// Apply platform-independent path rules before joining a destination path.
pub(crate) fn relative(raw: &str, max_depth: usize) -> Result<PathBuf> {
  if raw.is_empty() || raw.contains(['\\', ':']) || raw.chars().any(char::is_control) {
    return Err(SkillError::UnsafePath);
  }
  let mut output = PathBuf::new();
  let mut depth = 0;
  for part in raw.split('/') {
    depth += 1;
    let stem = part
      .split('.')
      .next()
      .unwrap_or_default()
      .to_ascii_uppercase();
    if part.is_empty()
      || part == "."
      || part == ".."
      || part.len() > 255
      || part.ends_with(['.', ' '])
      || part.contains(['<', '>', '"', '|', '?', '*'])
      || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
      || (stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
      return Err(SkillError::UnsafePath);
    }
    output.push(part);
  }
  if depth > max_depth {
    return Err(SkillError::Limit("directory depth"));
  }
  Ok(output)
}

pub(crate) fn unique(paths: &mut BTreeSet<String>, raw: &str) -> Result<()> {
  if !paths.insert(raw.to_lowercase()) {
    return Err(SkillError::UnsafePath);
  }
  Ok(())
}
