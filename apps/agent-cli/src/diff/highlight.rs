//! Syntax highlighting for diff lines through tree-sitter.
//!
//! Highlighting is presentation only: it never changes the text the client sends to the daemon.
//! Each span carries a byte range relative to its own line so renderers can slice safely.

use std::sync::OnceLock;

use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Syntax {
  Rust,
  Python,
  JavaScript,
  TypeScript,
  Json,
  Toml,
  Bash,
  Markdown,
  #[default]
  Plain,
}

impl Syntax {
  /// Language detection is extension based; unknown files stay plain rather than guessing.
  pub fn from_path(path: &str) -> Self {
    let extension = std::path::Path::new(path)
      .extension()
      .and_then(|value| value.to_str())
      .unwrap_or_default()
      .to_ascii_lowercase();
    match extension.as_str() {
      "rs" => Self::Rust,
      "py" | "pyi" => Self::Python,
      "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
      "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
      "json" | "jsonc" => Self::Json,
      "toml" => Self::Toml,
      "sh" | "bash" | "zsh" => Self::Bash,
      "md" | "markdown" => Self::Markdown,
      _ => Self::Plain,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
  Attribute,
  Comment,
  Constant,
  Function,
  Keyword,
  Number,
  Operator,
  Property,
  Punctuation,
  String,
  Type,
  Variable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HighlightSpan {
  /// Byte offset from the start of the line.
  pub start: usize,
  /// Byte offset one past the last byte, from the start of the line.
  pub end: usize,
  pub capture: Capture,
}

/// Highlight a contiguous run of lines, returning one span list per input line.
///
/// Callers pass a whole file section so multi-line strings and comments highlight correctly;
/// multi-line spans are clipped to the line they start on.
pub fn highlight_lines(syntax: Syntax, lines: &[&str]) -> Vec<Vec<HighlightSpan>> {
  let mut highlighted = vec![Vec::new(); lines.len()];
  if lines.is_empty() {
    return highlighted;
  }
  let Some(grammar) = grammar(syntax) else {
    return highlighted;
  };
  let mut parser = Parser::new();
  if parser.set_language(&grammar.language).is_err() {
    return highlighted;
  }
  let text = lines.join("\n");
  let Some(tree) = parser.parse(&text, None) else {
    return highlighted;
  };

  let starts = line_starts(lines);
  let mut candidates: Vec<Vec<Candidate>> = vec![Vec::new(); lines.len()];
  collect(
    grammar,
    tree.root_node(),
    &text,
    0,
    &starts,
    lines,
    &mut candidates,
  );

  // Markdown: the block tree only marks headings, fences and link targets, so each inline node
  // is parsed again with the inline grammar to colour emphasis, code spans and links.
  if let Some(inline) = &grammar.inline {
    let mut ranges = Vec::new();
    collect_inline_ranges(tree.root_node(), &mut ranges);
    let mut inline_parser = Parser::new();
    if inline_parser.set_language(&inline.language).is_ok() {
      for (start, end) in ranges {
        let Some(slice) = text.get(start..end) else {
          continue;
        };
        let Some(subtree) = inline_parser.parse(slice, None) else {
          continue;
        };
        collect(
          inline,
          subtree.root_node(),
          slice,
          start,
          &starts,
          lines,
          &mut candidates,
        );
      }
    }
  }

  for (target, found) in highlighted.iter_mut().zip(candidates.iter_mut()) {
    *target = resolve(found);
  }
  highlighted
}

/// Runs one compiled query over `source`, shifting match offsets by `offset`.
fn collect(
  grammar: &Grammar,
  root: Node<'_>,
  source: &str,
  offset: usize,
  starts: &[usize],
  lines: &[&str],
  candidates: &mut [Vec<Candidate>],
) {
  let mut cursor = QueryCursor::new();
  let mut matches = cursor.matches(&grammar.query, root, source.as_bytes());
  while let Some(matched) = matches.next() {
    for capture in matched.captures {
      let Some(mapped) = grammar
        .captures
        .get(capture.index as usize)
        .copied()
        .flatten()
      else {
        continue;
      };
      let node = capture.node;
      push_candidate(
        candidates,
        starts,
        lines,
        offset.saturating_add(node.start_byte()),
        offset.saturating_add(node.end_byte()),
        mapped,
      );
    }
  }
}

/// Collects the byte ranges of every `inline` node; an explicit stack keeps deep files safe.
fn collect_inline_ranges(root: Node<'_>, ranges: &mut Vec<(usize, usize)>) {
  let mut stack = vec![root];
  while let Some(node) = stack.pop() {
    if node.kind() == "inline" {
      ranges.push((node.start_byte(), node.end_byte()));
      continue;
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    stack.extend(children);
  }
}

/// A compiled query plus the capture-id table derived from it; built once per language.
struct Grammar {
  language: Language,
  query: Query,
  /// Indexed like `query.capture_names()`; `None` means the capture is not rendered.
  captures: Vec<Option<Mapped>>,
  /// Markdown only: `tree_sitter_md` keeps block and inline syntax in separate grammars.
  inline: Option<Box<Grammar>>,
}

/// A capture the renderer knows about, with the specificity used to resolve overlaps.
#[derive(Clone, Copy)]
struct Mapped {
  capture: Capture,
  /// Longer names are more specific, so `function.method` beats `variable` at the same start.
  priority: u8,
}

impl Grammar {
  /// Compiling the bundled highlights query is the only failure point; callers fall back to
  /// empty spans, and grammars without a highlights query simply never register here.
  fn compile(language: Language, source: &str) -> Option<Self> {
    let query = Query::new(&language, source).ok()?;
    let captures = query
      .capture_names()
      .iter()
      .map(|name| {
        map_capture(name).map(|capture| Mapped {
          capture,
          priority: capture_priority(name),
        })
      })
      .collect();
    Some(Self {
      language,
      query,
      captures,
      inline: None,
    })
  }
}

/// Markdown wraps a whole span in `text.*` while the delimiters inside it are `punctuation.*`,
/// so name length alone would let the delimiter win and leave the words uncoloured.
fn capture_priority(name: &str) -> u8 {
  if name.starts_with("text.") {
    u8::MAX
  } else {
    u8::try_from(name.len()).unwrap_or(u8::MAX)
  }
}

/// Captures from the usual `highlights.scm` vocabulary, keyed by the first dotted segment.
const CAPTURE_NAMES: &[(&str, Capture)] = &[
  ("attribute", Capture::Attribute),
  ("boolean", Capture::Constant),
  ("bracket", Capture::Punctuation),
  ("comment", Capture::Comment),
  ("constant", Capture::Constant),
  ("constructor", Capture::Function),
  ("delimiter", Capture::Punctuation),
  ("function", Capture::Function),
  ("keyword", Capture::Keyword),
  ("number", Capture::Number),
  ("operator", Capture::Operator),
  ("property", Capture::Property),
  ("punctuation", Capture::Punctuation),
  ("string", Capture::String),
  ("type", Capture::Type),
  ("variable", Capture::Variable),
];

fn map_capture(name: &str) -> Option<Capture> {
  // Markdown routes everything through `text.*`, so the second segment carries the meaning.
  if let Some(rest) = name.strip_prefix("text.") {
    return match rest.split('.').next().unwrap_or(rest) {
      "title" => Some(Capture::Keyword),
      "literal" => Some(Capture::String),
      "uri" => Some(Capture::Property),
      "reference" => Some(Capture::Function),
      "emphasis" | "strong" => Some(Capture::Type),
      "escape" => Some(Capture::Constant),
      _ => None,
    };
  }
  // `keyword.function` -> `keyword`, `punctuation.bracket` -> `punctuation`, and so on.
  let head = name.split('.').next().unwrap_or(name);
  CAPTURE_NAMES
    .iter()
    .find(|(key, _)| *key == head)
    .map(|(_, capture)| *capture)
}

macro_rules! cached_grammar {
  ($name:ident, $krate:ident, $language:ident, $query:ident) => {
    fn $name() -> Option<&'static Grammar> {
      static CACHE: OnceLock<Option<Grammar>> = OnceLock::new();
      CACHE
        .get_or_init(|| Grammar::compile($krate::$language.into(), $krate::$query))
        .as_ref()
    }
  };
}

cached_grammar!(rust_grammar, tree_sitter_rust, LANGUAGE, HIGHLIGHTS_QUERY);
cached_grammar!(
  python_grammar,
  tree_sitter_python,
  LANGUAGE,
  HIGHLIGHTS_QUERY
);
// `tree_sitter_javascript` and `tree_sitter_bash` spell the constant `HIGHLIGHT_QUERY`.
cached_grammar!(
  javascript_grammar,
  tree_sitter_javascript,
  LANGUAGE,
  HIGHLIGHT_QUERY
);
cached_grammar!(
  typescript_grammar,
  tree_sitter_typescript,
  LANGUAGE_TYPESCRIPT,
  HIGHLIGHTS_QUERY
);
cached_grammar!(json_grammar, tree_sitter_json, LANGUAGE, HIGHLIGHTS_QUERY);
cached_grammar!(
  toml_grammar,
  tree_sitter_toml_ng,
  LANGUAGE,
  HIGHLIGHTS_QUERY
);
cached_grammar!(bash_grammar, tree_sitter_bash, LANGUAGE, HIGHLIGHT_QUERY);

/// Markdown needs both halves: the block tree marks headings, fences and link targets, and each
/// inline node is parsed again so emphasis, code spans and links are coloured.
fn markdown_grammar() -> Option<&'static Grammar> {
  static CACHE: OnceLock<Option<Grammar>> = OnceLock::new();
  CACHE
    .get_or_init(|| {
      let mut grammar = Grammar::compile(
        tree_sitter_md::LANGUAGE.into(),
        tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
      )?;
      grammar.inline = Grammar::compile(
        tree_sitter_md::INLINE_LANGUAGE.into(),
        tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
      )
      .map(Box::new);
      Some(grammar)
    })
    .as_ref()
}

fn grammar(syntax: Syntax) -> Option<&'static Grammar> {
  match syntax {
    Syntax::Rust => rust_grammar(),
    Syntax::Python => python_grammar(),
    Syntax::JavaScript => javascript_grammar(),
    Syntax::TypeScript => typescript_grammar(),
    Syntax::Json => json_grammar(),
    Syntax::Toml => toml_grammar(),
    Syntax::Bash => bash_grammar(),
    Syntax::Markdown => markdown_grammar(),
    Syntax::Plain => None,
  }
}

#[derive(Clone, Copy)]
struct Candidate {
  start: usize,
  end: usize,
  mapped: Mapped,
}

/// Byte offset of each line inside `lines.join("\n")`.
fn line_starts(lines: &[&str]) -> Vec<usize> {
  let mut starts = Vec::with_capacity(lines.len());
  let mut offset = 0;
  for line in lines {
    starts.push(offset);
    offset += line.len() + 1;
  }
  starts
}

fn push_candidate(
  candidates: &mut [Vec<Candidate>],
  starts: &[usize],
  lines: &[&str],
  start_byte: usize,
  end_byte: usize,
  mapped: Mapped,
) {
  let index = starts
    .partition_point(|start| *start <= start_byte)
    .saturating_sub(1);
  let (Some(line), Some(&line_start)) = (lines.get(index), starts.get(index)) else {
    return;
  };
  let start = start_byte.saturating_sub(line_start);
  // Multi-line strings and comments are clipped to the line they start on.
  let end = end_byte.saturating_sub(line_start).min(line.len());
  if start >= end || !line.is_char_boundary(start) || !line.is_char_boundary(end) {
    return;
  }
  if let Some(bucket) = candidates.get_mut(index) {
    bucket.push(Candidate { start, end, mapped });
  }
}

/// Order spans by position, drop overlaps and zero-width ranges, and keep the most specific
/// capture for coinciding starts.
fn resolve(candidates: &mut [Candidate]) -> Vec<HighlightSpan> {
  candidates.sort_unstable_by(|left, right| {
    left
      .start
      .cmp(&right.start)
      .then(right.mapped.priority.cmp(&left.mapped.priority))
      .then(right.end.cmp(&left.end))
  });
  let mut spans = Vec::with_capacity(candidates.len());
  let mut filled = 0;
  for candidate in candidates.iter() {
    if candidate.start < filled {
      continue;
    }
    filled = candidate.end;
    spans.push(HighlightSpan {
      start: candidate.start,
      end: candidate.end,
      capture: candidate.mapped.capture,
    });
  }
  spans
}

#[cfg(test)]
mod tests {
  use super::*;

  fn captured<'a>(line: &'a str, spans: &[HighlightSpan]) -> Vec<(&'a str, Capture)> {
    spans
      .iter()
      .map(|span| (&line[span.start..span.end], span.capture))
      .collect()
  }

  fn assert_inside(line: &str, spans: &[HighlightSpan]) {
    for span in spans {
      assert!(span.start < span.end, "empty span {span:?}");
      assert!(span.end <= line.len(), "span past line end {span:?}");
      assert!(
        line.is_char_boundary(span.start),
        "unaligned start {span:?}"
      );
      assert!(line.is_char_boundary(span.end), "unaligned end {span:?}");
    }
    assert!(
      spans.windows(2).all(|pair| pair[0].end <= pair[1].start),
      "spans overlap or are unsorted: {spans:?}"
    );
  }

  #[test]
  fn detects_languages_from_extensions() {
    assert_eq!(Syntax::from_path("src/main.rs"), Syntax::Rust);
    assert_eq!(Syntax::from_path("app.tsx"), Syntax::TypeScript);
    assert_eq!(Syntax::from_path("Cargo.toml"), Syntax::Toml);
    assert_eq!(Syntax::from_path("README.md"), Syntax::Markdown);
    assert_eq!(Syntax::from_path("deploy.bash"), Syntax::Bash);
    assert_eq!(Syntax::from_path("LICENSE"), Syntax::Plain);
  }

  #[test]
  fn highlights_rust_keywords_strings_comments_and_functions() {
    let lines = [
      "fn main() {",
      r#"  let text = "hello";"#,
      "  // explain",
      "}",
    ];
    let spans = highlight_lines(Syntax::Rust, &lines);
    assert_eq!(spans.len(), lines.len());
    for (line, line_spans) in lines.iter().zip(&spans) {
      assert_inside(line, line_spans);
    }

    let first = captured(lines[0], &spans[0]);
    assert!(first.contains(&("fn", Capture::Keyword)), "{first:?}");
    assert!(first.contains(&("main", Capture::Function)), "{first:?}");

    let second = captured(lines[1], &spans[1]);
    assert!(second.contains(&("let", Capture::Keyword)), "{second:?}");
    assert!(
      second.contains(&("\"hello\"", Capture::String)),
      "{second:?}"
    );
    assert!(second.contains(&(";", Capture::Punctuation)), "{second:?}");

    let third = captured(lines[2], &spans[2]);
    assert!(
      third.contains(&("// explain", Capture::Comment)),
      "{third:?}"
    );
  }

  #[test]
  fn spans_stay_on_char_boundaries_for_non_ascii_lines() {
    let lines = [
      r#"let s = "héllo 世界";"#,
      "// 注释 ünicode",
      "fn 名字() -> u8 { 0 }",
    ];
    let spans = highlight_lines(Syntax::Rust, &lines);
    assert_eq!(spans.len(), lines.len());
    for (line, line_spans) in lines.iter().zip(&spans) {
      assert_inside(line, line_spans);
    }
    let string = captured(lines[0], &spans[0]);
    assert!(
      string.contains(&("\"héllo 世界\"", Capture::String)),
      "{string:?}"
    );
  }

  #[test]
  fn markdown_colours_headings_emphasis_and_links() {
    // Markdown needs the block grammar for headings and fences plus the inline grammar for
    // emphasis and links; a single pass would leave the words uncoloured.
    let lines = [
      "# 标题",
      "普通 **粗体** 和 [链接](https://example.com)。",
      "```",
      "fn main() {}",
      "```",
    ];
    let spans = highlight_lines(Syntax::Markdown, &lines);
    assert_eq!(spans.len(), lines.len());
    assert!(
      spans[0].iter().any(|span| span.capture == Capture::Keyword),
      "heading text is a title capture"
    );
    assert!(
      spans[0]
        .iter()
        .any(|span| span.capture == Capture::Punctuation),
      "the leading # stays punctuation"
    );
    assert!(
      spans[1].iter().any(|span| span.capture == Capture::Type),
      "strong emphasis colours its words"
    );
    assert!(
      spans[1]
        .iter()
        .any(|span| span.capture == Capture::Function),
      "link text is a reference"
    );
    assert!(
      spans[1]
        .iter()
        .any(|span| span.capture == Capture::Property),
      "link destination is a uri"
    );
    assert!(
      spans[2].iter().any(|span| span.capture == Capture::String),
      "a fence opens a literal block"
    );
    for (line, found) in lines.iter().zip(&spans) {
      for span in found {
        assert!(span.start < span.end, "spans are never empty");
        assert!(span.end <= line.len(), "spans stay inside their line");
        assert!(
          line.is_char_boundary(span.start) && line.is_char_boundary(span.end),
          "spans land on character boundaries"
        );
      }
    }
  }

  #[test]
  fn plain_markdown_without_markup_produces_no_spans() {
    let spans = highlight_lines(Syntax::Markdown, &["just words", "", "and more"]);
    assert!(spans.iter().all(Vec::is_empty));
  }

  #[test]
  fn clips_multiline_comment_to_its_first_line() {
    let lines = ["/* leading", "middle line", "trailing */ let x = 1;"];
    let spans = highlight_lines(Syntax::Rust, &lines);
    assert_eq!(spans.len(), lines.len());
    assert_eq!(
      spans[0],
      vec![HighlightSpan {
        start: 0,
        end: lines[0].len(),
        capture: Capture::Comment,
      }]
    );
    assert!(spans[1].is_empty(), "{:?}", spans[1]);
    assert!(
      !captured(lines[2], &spans[2])
        .iter()
        .any(|(_, capture)| *capture == Capture::Comment),
      "{:?}",
      spans[2]
    );
    for (line, line_spans) in lines.iter().zip(&spans) {
      assert_inside(line, line_spans);
    }
  }

  #[test]
  fn clips_multiline_string_to_its_first_line() {
    let lines = ["let text = r#\"first", "second\"#;"];
    let spans = highlight_lines(Syntax::Rust, &lines);
    assert_eq!(spans.len(), lines.len());
    let first = captured(lines[0], &spans[0]);
    assert!(
      first
        .iter()
        .any(|(text, capture)| *capture == Capture::String && *text == "r#\"first"),
      "{first:?}"
    );
    assert!(
      !captured(lines[1], &spans[1])
        .iter()
        .any(|(_, capture)| *capture == Capture::String),
      "{:?}",
      spans[1]
    );
    for (line, line_spans) in lines.iter().zip(&spans) {
      assert_inside(line, line_spans);
    }
  }

  #[test]
  fn plain_syntax_returns_no_spans() {
    let lines = ["fn main() {}", "let value = 1;"];
    let spans = highlight_lines(Syntax::Plain, &lines);
    assert_eq!(spans, vec![Vec::new(), Vec::new()]);
  }

  #[test]
  fn degenerate_input_does_not_panic() {
    assert!(highlight_lines(Syntax::Rust, &[]).is_empty());
    let all = [
      Syntax::Rust,
      Syntax::Python,
      Syntax::JavaScript,
      Syntax::TypeScript,
      Syntax::Json,
      Syntax::Toml,
      Syntax::Bash,
      Syntax::Markdown,
      Syntax::Plain,
    ];
    for syntax in all {
      for lines in [&[""][..], &["   ", "", "  "][..], &["\u{1f600}"][..]] {
        let spans = highlight_lines(syntax, lines);
        assert_eq!(spans.len(), lines.len());
        for (line, line_spans) in lines.iter().zip(&spans) {
          assert_inside(line, line_spans);
        }
      }
    }
  }

  #[test]
  fn other_grammars_produce_spans() {
    let python = highlight_lines(Syntax::Python, &["def run():", "  return 1"]);
    assert!(
      captured("def run():", &python[0]).contains(&("def", Capture::Keyword)),
      "{:?}",
      python[0]
    );

    let json = highlight_lines(Syntax::Json, &[r#"{"key": true}"#]);
    assert!(
      captured(r#"{"key": true}"#, &json[0]).contains(&("true", Capture::Constant)),
      "{:?}",
      json[0]
    );
  }
}
