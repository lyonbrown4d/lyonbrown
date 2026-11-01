use agent_workspace::Workspace;

#[cfg(unix)]
#[test]
fn discovery_keeps_literal_backslash_names_in_the_selected_directory() {
  let directory = tempfile::tempdir().unwrap();
  let literal = directory.path().join(r"back\slash");
  std::fs::create_dir(&literal).unwrap();
  std::fs::write(literal.join("inside.txt"), "selected needle").unwrap();
  std::fs::create_dir_all(directory.path().join("back/slash")).unwrap();
  std::fs::write(
    directory.path().join("back/slash/inside.txt"),
    "wrong needle",
  )
  .unwrap();
  let workspace = Workspace::open(directory.path()).unwrap();
  let expected = r"back\slash/inside.txt";
  assert_eq!(workspace.list(r"back\slash").unwrap()[0].path, expected);
  let listing = workspace.list_files(r"back\slash", 4, 200).unwrap();
  assert_eq!(listing.entries[0].path, expected);
  let search = workspace
    .search_files(r"back\slash", "needle", 4, 200)
    .unwrap();
  assert_eq!(search.matches.len(), 1);
  assert_eq!(search.matches[0].path, expected);
  assert_eq!(search.matches[0].text, "selected needle");
}

#[test]
fn discovery_search_reports_depth_results_and_content_omissions() {
  let root = tempfile::tempdir().unwrap();
  let workspace = Workspace::open(root.path()).unwrap();
  workspace
    .write("src/a.txt", "needle first\nother\nneedle last")
    .unwrap();
  workspace.write("src/deep/file.txt", "needle deep").unwrap();
  workspace
    .write("target/ignored.txt", "needle generated")
    .unwrap();
  std::fs::write(root.path().join("binary"), b"needle\0binary").unwrap();
  std::fs::write(root.path().join("invalid"), b"needle\xff").unwrap();
  std::fs::write(root.path().join("large"), vec![b'a'; 1_048_577]).unwrap();
  let listing = workspace.list_files("", 1, 200).unwrap();
  assert!(
    listing
      .entries
      .iter()
      .any(|entry| entry.path == "src" && entry.directory)
  );
  assert!(
    !listing
      .entries
      .iter()
      .any(|entry| entry.path == "src/a.txt")
  );
  assert!(listing.report.truncated);
  assert_eq!(listing.report.depth_limited, 1);
  assert_eq!(listing.report.skipped_directories, 1);
  let search = workspace.search_files("", "needle", 4, 200).unwrap();
  assert_eq!(
    search
      .matches
      .iter()
      .map(|m| (m.path.as_str(), m.line, m.text.as_str()))
      .collect::<Vec<_>>(),
    vec![
      ("src/a.txt", 1, "needle first"),
      ("src/a.txt", 3, "needle last"),
      ("src/deep/file.txt", 1, "needle deep")
    ]
  );
  assert!(search.report.truncated);
  assert_eq!(search.report.skipped_binary, 1);
  assert_eq!(search.report.skipped_non_utf8, 1);
  assert_eq!(search.report.skipped_large, 1);
  assert_eq!(search.report.searched_files, 2);
  let bounded = workspace.search_files("src", "needle", 4, 1).unwrap();
  assert_eq!(bounded.matches.len(), 1);
  assert!(bounded.report.result_limit_reached);
  assert!(bounded.report.truncated);
  assert!(workspace.list_files("../", 1, 200).is_err());
  assert!(workspace.list_files("", 9, 200).is_err());
  assert!(workspace.search_files("", "", 4, 200).is_err());
  assert!(workspace.search_files("", "needle", 4, 201).is_err());
}

#[test]
fn discovery_budgets_bound_output_and_total_scanned_content() {
  let root = tempfile::tempdir().unwrap();
  let workspace = Workspace::open(root.path()).unwrap();
  for index in 0..10 {
    workspace
      .write(&format!("{index}.txt"), &"x".repeat(1_048_576))
      .unwrap();
  }
  let search = workspace.search_files("", "absent", 4, 200).unwrap();
  assert!(search.matches.is_empty());
  assert_eq!(search.report.searched_files, 8);
  assert_eq!(search.report.read_bytes, 8 * 1_048_576);
  assert!(search.report.content_limit_reached);
  assert!(search.report.truncated);
  workspace
    .write("0.txt", &format!("needle{}", "x".repeat(100_000)))
    .unwrap();
  let search = workspace.search_files("", "needle", 4, 200).unwrap();
  assert!(
    search.matches.is_empty(),
    "oversize result line must not exceed the output bound"
  );
  assert!(search.report.output_limit_reached);
}

#[cfg(unix)]
#[test]
fn discovery_never_searches_symlink_escapes() {
  let root = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  std::fs::write(outside.path().join("secret"), "outside needle").unwrap();
  std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
  std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("file")).unwrap();
  let workspace = Workspace::open(root.path()).unwrap();
  let listing = workspace.list_files("", 4, 200).unwrap();
  assert!(listing.entries.is_empty());
  assert_eq!(listing.report.skipped_symlinks, 2);
  let search = workspace.search_files("", "needle", 4, 200).unwrap();
  assert!(search.matches.is_empty());
  assert_eq!(search.report.skipped_symlinks, 2);
  assert!(workspace.search_files("escape", "needle", 4, 200).is_err());
}

#[test]
fn content_search_limits_total_visited_entries_even_without_any_matches() {
  let root = tempfile::tempdir().unwrap();
  for index in 0..2001 {
    std::fs::write(root.path().join(format!("{index}.txt")), "fixture").unwrap();
  }
  let workspace = Workspace::open(root.path()).unwrap();
  let search = workspace.search_files("", "absent", 4, 200).unwrap();
  assert!(search.matches.is_empty());
  assert_eq!(search.report.visited_entries, 2000);
  assert_eq!(search.report.searched_files, 2000);
  assert!(search.report.entry_limit_reached);
  assert!(search.report.truncated);
}
