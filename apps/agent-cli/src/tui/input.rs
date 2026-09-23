use crossterm::event::KeyCode;
use unicode_segmentation::UnicodeSegmentation;

pub fn edit_input(input: &str, cursor: usize, key: KeyCode) -> (String, usize) {
  let cursor = floor_grapheme_boundary(input, cursor.min(input.len()));
  let mut edited = input.to_owned();
  match key {
    KeyCode::Left => return (edited, previous_boundary(input, cursor)),
    KeyCode::Right => return (edited, next_boundary(input, cursor)),
    KeyCode::Home => return (edited, 0),
    KeyCode::End => return (edited, input.len()),
    KeyCode::Backspace if cursor > 0 => {
      let previous = previous_boundary(input, cursor);
      edited.replace_range(previous..cursor, "");
      return (edited, previous);
    }
    KeyCode::Delete if cursor < input.len() => {
      edited.replace_range(cursor..next_boundary(input, cursor), "");
    }
    KeyCode::Char(character) => {
      edited.insert(cursor, character);
      let inserted_end = cursor + character.len_utf8();
      let cursor = ceil_grapheme_boundary(&edited, inserted_end);
      return (edited, cursor);
    }
    _ => {}
  }
  (edited, cursor)
}

pub fn next_conversation_scroll(current: u16, max_scroll: u16, key: KeyCode) -> u16 {
  let next = match key {
    KeyCode::Up => current.saturating_add(1),
    KeyCode::Down => current.saturating_sub(1),
    KeyCode::PageUp => current.saturating_add(5),
    KeyCode::PageDown => current.saturating_sub(5),
    _ => current,
  };
  next.min(max_scroll)
}

fn floor_grapheme_boundary(value: &str, index: usize) -> usize {
  if index == value.len() {
    return index;
  }
  value
    .grapheme_indices(true)
    .map(|(offset, _)| offset)
    .take_while(|offset| *offset <= index)
    .last()
    .unwrap_or(0)
}

fn ceil_grapheme_boundary(value: &str, index: usize) -> usize {
  value
    .grapheme_indices(true)
    .map(|(offset, _)| offset)
    .find(|offset| *offset >= index)
    .unwrap_or(value.len())
}

fn previous_boundary(value: &str, cursor: usize) -> usize {
  value
    .grapheme_indices(true)
    .map(|(offset, _)| offset)
    .take_while(|offset| *offset < cursor)
    .last()
    .unwrap_or(0)
}

fn next_boundary(value: &str, cursor: usize) -> usize {
  value
    .grapheme_indices(true)
    .map(|(offset, _)| offset)
    .find(|offset| *offset > cursor)
    .unwrap_or(value.len())
}
