pub(super) fn contains_trigger(prompt: &str, trigger: &str) -> bool {
  let prompt = prompt.to_lowercase();
  let trigger = trigger.trim().to_lowercase();
  if trigger.is_empty() {
    return false;
  }

  prompt.match_indices(&trigger).any(|(start, matched)| {
    let end = start + matched.len();
    let left_boundary = prompt[..start]
      .chars()
      .next_back()
      .is_none_or(|character| !is_word_character(character));
    let right_boundary = prompt[end..]
      .chars()
      .next()
      .is_none_or(|character| !is_word_character(character));
    left_boundary && right_boundary
  })
}

fn is_word_character(character: char) -> bool {
  character.is_ascii_alphanumeric() || character == '_'
}
