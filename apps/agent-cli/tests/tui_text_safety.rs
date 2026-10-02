use agent_cli::tui::{status_line_safe, transcript_safe};

#[test]
fn status_text_is_single_line_and_strips_terminal_and_bidi_formatting() {
  let dirty = concat!(
    "\u{1b}[31mred\u{1b}[0m\nnext\t\u{2028}",
    "\u{1b}Psecret\u{1b}\\\u{202e}spoof\u{2066}x\u{2069}"
  );

  assert_eq!(status_line_safe(dirty), "red next  �spoof�x�");
}

#[test]
fn transcript_text_keeps_layout_but_strips_terminal_and_bidi_formatting() {
  let dirty = "\u{1b}]0;title\u{7}first\n\tsecond\u{202e}";

  assert_eq!(transcript_safe(dirty), "first\n\tsecond�");
}
