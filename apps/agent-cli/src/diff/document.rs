//! Parse the daemon's unified diff text into the presentation model.

use super::model::{DiffDocument, DiffLine, FileDiff, Hunk, LineKind};

/// A hunk still collecting lines, together with its running line cursors. The header keeps the
/// raw `@@ ... @@` line so the view can show its context.
struct OpenHunk {
  hunk: Hunk,
  old_line: u32,
  new_line: u32,
}

impl OpenHunk {
  fn new(header: String, old_start: u32, new_start: u32) -> Self {
    Self {
      hunk: Hunk {
        header,
        old_start,
        new_start,
        lines: Vec::new(),
      },
      old_line: old_start,
      new_line: new_start,
    }
  }

  fn push(&mut self, kind: LineKind, text: &str) {
    let (old_line, new_line) = match kind {
      LineKind::Context => (Some(self.old_line), Some(self.new_line)),
      LineKind::Removed => (Some(self.old_line), None),
      LineKind::Added => (None, Some(self.new_line)),
    };
    self.hunk.lines.push(DiffLine {
      kind,
      old_line,
      new_line,
      text: text.to_string(),
    });
    if kind != LineKind::Added {
      self.old_line = self.old_line.saturating_add(1);
    }
    if kind != LineKind::Removed {
      self.new_line = self.new_line.saturating_add(1);
    }
  }
}

/// Parses `git diff` output. Unknown headers are skipped rather than rejected, so a new git
/// feature never blanks the whole view. Binary patches and mode-only changes keep their paths
/// and contribute no hunks.
pub fn parse(text: &str) -> DiffDocument {
  let mut document = DiffDocument::default();
  let mut file: Option<FileDiff> = None;
  let mut open: Option<OpenHunk> = None;
  let mut binary_patch = false;

  for raw in text.split('\n') {
    let line = raw.strip_suffix('\r').unwrap_or(raw);

    // A file header always wins, even inside a binary patch payload.
    if let Some(rest) = line.strip_prefix("diff --git ") {
      flush_file(&mut document, &mut file, &mut open);
      let (old_path, new_path) = split_git_paths(rest).unwrap_or_default();
      file = Some(FileDiff {
        old_path,
        new_path,
        hunks: Vec::new(),
      });
      binary_patch = false;
      continue;
    }
    if binary_patch {
      continue;
    }
    if line.starts_with("GIT binary patch") {
      binary_patch = true;
      continue;
    }

    // Inside a hunk only ` `, `+`, `-` and the `\` marker carry meaning; a new `@@` header ends
    // the hunk, anything else is ignored without moving the cursors.
    if open.is_some() {
      if let Some(open_hunk) = open.as_mut()
        && let Some((kind, content)) = split_body_line(line)
      {
        open_hunk.push(kind, content);
        continue;
      }
      if !line.starts_with("@@") {
        continue;
      }
      flush_hunk(&mut file, &mut open);
    }

    if let Some((old_start, new_start)) = parse_hunk_header(line) {
      if file.is_some() {
        open = Some(OpenHunk::new(
          line.trim_end().to_string(),
          old_start,
          new_start,
        ));
      }
      continue;
    }
    if let Some(rest) = line.strip_prefix("--- ") {
      file.get_or_insert_with(FileDiff::default).old_path = strip_path_prefix(rest.trim_end());
      continue;
    }
    if let Some(rest) = line.strip_prefix("+++ ") {
      file.get_or_insert_with(FileDiff::default).new_path = strip_path_prefix(rest.trim_end());
      continue;
    }
  }

  flush_file(&mut document, &mut file, &mut open);
  document
}

fn flush_hunk(file: &mut Option<FileDiff>, open: &mut Option<OpenHunk>) {
  if let (Some(file), Some(open)) = (file.as_mut(), open.take()) {
    file.hunks.push(open.hunk);
  }
}

fn flush_file(
  document: &mut DiffDocument,
  file: &mut Option<FileDiff>,
  open: &mut Option<OpenHunk>,
) {
  flush_hunk(file, open);
  if let Some(file) = file.take() {
    document.files.push(file);
  }
}

/// Splits a body line into its kind and the text after the single prefix character.
fn split_body_line(line: &str) -> Option<(LineKind, &str)> {
  let kind = match line.as_bytes().first()? {
    b' ' => LineKind::Context,
    b'+' => LineKind::Added,
    b'-' => LineKind::Removed,
    _ => return None,
  };
  Some((kind, line.get(1..)?))
}

/// Parses `@@ -old_start[,old_count] +new_start[,new_count] @@` and returns both start lines.
/// Counts are optional in git output and unnecessary here: numbering follows the body lines.
fn parse_hunk_header(line: &str) -> Option<(u32, u32)> {
  let (old_range, rest) = line.strip_prefix("@@ -")?.split_once(' ')?;
  let rest = rest.strip_prefix('+')?;
  let (new_range, _) = rest.split_once(" @@").unwrap_or((rest, ""));
  Some((parse_range_start(old_range)?, parse_range_start(new_range)?))
}

fn parse_range_start(range: &str) -> Option<u32> {
  range.split(',').next()?.parse().ok()
}

/// Drops the prefix git puts on diff paths; `/dev/null` marks an added or deleted file.
fn strip_path_prefix(path: &str) -> String {
  if path == "/dev/null" {
    return path.to_string();
  }
  path
    .strip_prefix("a/")
    .or_else(|| path.strip_prefix("b/"))
    .unwrap_or(path)
    .to_string()
}

/// Splits the two paths of `diff --git a/x b/y`. Git leaves spaces unquoted, so prefer the
/// boundary that names the same path twice; a rename falls back to the first ` b/` boundary.
fn split_git_paths(rest: &str) -> Option<(String, String)> {
  let mut fallback = None;
  for (index, _) in rest.match_indices(" b/") {
    if !rest[..index].starts_with("a/") {
      continue;
    }
    let (old_path, new_path) = (
      strip_path_prefix(&rest[..index]),
      strip_path_prefix(&rest[index + 3..]),
    );
    if old_path == new_path {
      return Some((old_path, new_path));
    }
    fallback.get_or_insert((old_path, new_path));
  }
  fallback
}

#[cfg(test)]
mod tests {
  use super::*;

  fn heads(hunk: &Hunk) -> Vec<(LineKind, Option<u32>, Option<u32>)> {
    hunk
      .lines
      .iter()
      .map(|line| (line.kind, line.old_line, line.new_line))
      .collect()
  }

  #[test]
  fn parses_single_file_with_one_hunk() {
    let diff = "diff --git a/a.rs b/a.rs\nindex 1..2 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -1,3 +1,4 @@\n context\n-removed\n+added\n more\n";
    let document = parse(diff);
    assert_eq!(document.files.len(), 1);
    let file = &document.files[0];
    assert_eq!(file.new_path, "a.rs");
    assert_eq!(file.hunks.len(), 1);
    let lines: Vec<LineKind> = file.hunks[0].lines.iter().map(|line| line.kind).collect();
    assert_eq!(
      lines,
      vec![
        LineKind::Context,
        LineKind::Removed,
        LineKind::Added,
        LineKind::Context
      ]
    );
    assert_eq!(file.hunks[0].lines[1].old_line, Some(2));
    assert_eq!(file.hunks[0].lines[2].new_line, Some(2));
  }

  #[test]
  fn parses_multiple_files_with_multiple_hunks() {
    let diff = concat!(
      "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,2 @@ fn first()\n",
      "-old one\n+new one\n context one\n@@ -10,2 +10,3 @@ fn second()\n context two\n",
      "+added two\n context three\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n",
      "@@ -5 +5 @@\n-b\n+b\n",
    );
    let document = parse(diff);
    let (first, second) = (&document.files[0], &document.files[1]);
    assert_eq!(document.files.len(), 2);
    assert_eq!(
      (first.old_path.as_str(), first.new_path.as_str()),
      ("src/a.rs", "src/a.rs")
    );
    assert_eq!(second.new_path, "b.rs");
    assert_eq!(first.hunks.len(), 2);
    assert_eq!(first.hunks[0].header, "@@ -1,2 +1,2 @@ fn first()");
    assert_eq!((first.hunks[0].old_start, first.hunks[0].new_start), (1, 1));
    assert_eq!(
      heads(&first.hunks[0]),
      vec![
        (LineKind::Removed, Some(1), None),
        (LineKind::Added, None, Some(1)),
        (LineKind::Context, Some(2), Some(2))
      ]
    );
    assert_eq!(
      heads(&first.hunks[1]),
      vec![
        (LineKind::Context, Some(10), Some(10)),
        (LineKind::Added, None, Some(11)),
        (LineKind::Context, Some(11), Some(12))
      ]
    );
    assert_eq!(
      heads(&second.hunks[0]),
      vec![
        (LineKind::Removed, Some(5), None),
        (LineKind::Added, None, Some(5))
      ]
    );
    assert_eq!(document.stats().files, 2);
  }

  #[test]
  fn parses_added_file_from_dev_null() {
    let diff = "diff --git a/new.rs b/new.rs\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1,2 @@\n+  fn new() {\n+}\n";
    let document = parse(diff);
    let file = &document.files[0];
    assert_eq!(
      (file.old_path.as_str(), file.new_path.as_str()),
      ("/dev/null", "new.rs")
    );
    assert_eq!(file.display_path(), "new.rs");
    let lines = &file.hunks[0].lines;
    assert_eq!(lines.len(), 2);
    assert_eq!(
      (lines[0].old_line, lines[0].new_line, lines[0].text.as_str()),
      (None, Some(1), "  fn new() {")
    );
    assert_eq!(
      (lines[1].old_line, lines[1].new_line, lines[1].text.as_str()),
      (None, Some(2), "}")
    );
  }

  #[test]
  fn parses_deleted_file_into_dev_null() {
    let diff = "diff --git a/gone.rs b/gone.rs\ndeleted file mode 100644\n--- a/gone.rs\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-bye\n-bye again\n";
    let document = parse(diff);
    let file = &document.files[0];
    assert_eq!(
      (file.old_path.as_str(), file.new_path.as_str()),
      ("gone.rs", "/dev/null")
    );
    assert_eq!(file.display_path(), "gone.rs");
    let lines = &file.hunks[0].lines;
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0].old_line, lines[1].old_line), (Some(1), Some(2)));
    assert!(lines.iter().all(|line| line.new_line.is_none()));
  }

  #[test]
  fn skips_no_newline_markers() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
    let document = parse(diff);
    let lines = &document.files[0].hunks[0].lines;
    assert_eq!(lines.len(), 2);
    assert_eq!(
      (
        lines[0].kind,
        lines[0].text.as_str(),
        lines[1].kind,
        lines[1].text.as_str()
      ),
      (LineKind::Removed, "old", LineKind::Added, "new")
    );
    assert_eq!((lines[1].old_line, lines[1].new_line), (None, Some(1)));
  }

  #[test]
  fn accepts_hunk_headers_without_counts() {
    let diff = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -5 +7 @@ fn main()\n context\n+x\n";
    let document = parse(diff);
    let hunk = &document.files[0].hunks[0];
    assert_eq!((hunk.old_start, hunk.new_start), (5, 7));
    assert_eq!(hunk.header, "@@ -5 +7 @@ fn main()");
    assert_eq!(
      heads(hunk),
      vec![
        (LineKind::Context, Some(5), Some(7)),
        (LineKind::Added, None, Some(8))
      ]
    );
  }

  #[test]
  fn empty_input_yields_empty_document() {
    let document = parse("");
    assert!(document.files.is_empty());
    assert!(document.is_empty());
  }

  #[test]
  fn malformed_lines_are_ignored_without_shifting_numbers() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -2 +2 @@\n keep\nthis is not a diff line\n\n\\ No newline at end of file\n+added\n@@ broken header\nnoise\n";
    let document = parse(diff);
    assert_eq!(document.files.len(), 1);
    let hunk = &document.files[0].hunks[0];
    assert_eq!(
      heads(hunk),
      vec![
        (LineKind::Context, Some(2), Some(2)),
        (LineKind::Added, None, Some(3))
      ]
    );
    assert_eq!(hunk.lines[0].text, "keep");
  }

  #[test]
  fn keeps_metadata_only_files_without_hunks() {
    let diff = "diff --git a/mode.sh b/mode.sh\nold mode 100644\nnew mode 100755\ndiff --git a/old_name.rs b/new_name.rs\nsimilarity index 100%\nrename from old_name.rs\nrename to new_name.rs\ndiff --git a/img.png b/img.png\nnew file mode 100644\nGIT binary patch\nliteral 12\nRcmZ?wbhEHb\n\ndiff --git a/bin.dat b/bin.dat\nBinary files a/bin.dat and b/bin.dat differ\n";
    let document = parse(diff);
    let paths: Vec<(&str, &str)> = document
      .files
      .iter()
      .map(|file| (file.old_path.as_str(), file.new_path.as_str()))
      .collect();
    assert_eq!(
      paths,
      vec![
        ("mode.sh", "mode.sh"),
        ("old_name.rs", "new_name.rs"),
        ("img.png", "img.png"),
        ("bin.dat", "bin.dat")
      ]
    );
    assert!(document.files.iter().all(|file| file.hunks.is_empty()));
    assert!(document.is_empty());
  }
}
