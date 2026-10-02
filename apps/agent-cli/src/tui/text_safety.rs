#[derive(Clone, Copy)]
enum AnsiState {
  Text,
  Escape,
  EscapeIntermediate,
  Csi,
  Osc,
  OscEscape,
  ControlString,
  ControlStringEscape,
}

pub fn status_line_safe(value: &str) -> String {
  sanitize(value, false)
}

pub fn transcript_safe(value: &str) -> String {
  sanitize(value, true)
}

fn sanitize(value: &str, preserve_layout: bool) -> String {
  let mut output = String::with_capacity(value.len());
  let mut state = AnsiState::Text;
  for character in value.chars() {
    state = match state {
      AnsiState::Text => match character {
        '\u{1b}' => AnsiState::Escape,
        '\u{009b}' => AnsiState::Csi,
        '\u{009d}' => AnsiState::Osc,
        '\u{0090}' | '\u{0098}' | '\u{009e}' | '\u{009f}' => AnsiState::ControlString,
        character if is_bidi_formatting(character) => {
          output.push('\u{fffd}');
          AnsiState::Text
        }
        '\n' | '\t' if preserve_layout => {
          output.push(character);
          AnsiState::Text
        }
        '\n' | '\r' | '\t' | '\u{0085}' | '\u{2028}' | '\u{2029}' => {
          output.push(' ');
          AnsiState::Text
        }
        character if character.is_control() => {
          output.push('\u{fffd}');
          AnsiState::Text
        }
        character => {
          output.push(character);
          AnsiState::Text
        }
      },
      AnsiState::Escape => match character {
        '[' => AnsiState::Csi,
        ']' => AnsiState::Osc,
        'P' | 'X' | '^' | '_' => AnsiState::ControlString,
        ' '..='/' => AnsiState::EscapeIntermediate,
        _ => AnsiState::Text,
      },
      AnsiState::EscapeIntermediate => {
        if (' '..='/').contains(&character) {
          AnsiState::EscapeIntermediate
        } else {
          AnsiState::Text
        }
      }
      AnsiState::Csi => {
        if ('@'..='~').contains(&character) {
          AnsiState::Text
        } else {
          AnsiState::Csi
        }
      }
      AnsiState::Osc => match character {
        '\u{7}' | '\u{009c}' => AnsiState::Text,
        '\u{1b}' => AnsiState::OscEscape,
        _ => AnsiState::Osc,
      },
      AnsiState::OscEscape => {
        if character == '\\' {
          AnsiState::Text
        } else if character == '\u{1b}' {
          AnsiState::OscEscape
        } else {
          AnsiState::Osc
        }
      }
      AnsiState::ControlString => match character {
        '\u{009c}' => AnsiState::Text,
        '\u{1b}' => AnsiState::ControlStringEscape,
        _ => AnsiState::ControlString,
      },
      AnsiState::ControlStringEscape => {
        if character == '\\' {
          AnsiState::Text
        } else if character == '\u{1b}' {
          AnsiState::ControlStringEscape
        } else {
          AnsiState::ControlString
        }
      }
    };
  }
  output
}

fn is_bidi_formatting(character: char) -> bool {
  matches!(
    character,
    '\u{061c}'
      | '\u{200e}'
      | '\u{200f}'
      | '\u{202a}'..='\u{202e}'
      | '\u{2066}'..='\u{2069}'
  )
}
