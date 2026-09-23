use std::{fs, path::PathBuf, time::Duration};

use agent_index::WorkspaceIndex;
use uuid::Uuid;

struct TempWorkspace(PathBuf);

impl TempWorkspace {
  fn new() -> Self {
    let path = std::env::temp_dir().join(format!("lyonbrown-index-{}", Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    Self(path)
  }

  fn write(&self, relative: &str, content: &str) {
    let path = self.0.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
  }

  fn index_path(&self) -> PathBuf {
    self.0.join(".agent-index.db")
  }
}

impl Drop for TempWorkspace {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.0);
  }
}

fn open_index(workspace: &TempWorkspace) -> WorkspaceIndex {
  WorkspaceIndex::open(&workspace.0, workspace.index_path()).unwrap()
}

#[test]
fn initial_scan_indexes_supported_files_and_respects_gitignore() {
  let workspace = TempWorkspace::new();
  workspace.write(".gitignore", "ignored.rs\nsecret.env\nnode_modules/\n");
  workspace.write("src/main.rs", "fn main() { println!(\"needle\"); }");
  workspace.write("ignored.rs", "needle must not be indexed");
  workspace.write("secret.env", "needle=private");
  workspace.write("secrets.json", "{\"token\": \"needle-private\"}");
  workspace.write("node_modules/pkg/index.js", "needle dependency");
  workspace.write("binary.png", "not searchable needle");

  let index = open_index(&workspace);
  let results = index.search("needle", 10).unwrap();

  assert_eq!(results.len(), 1, "unexpected search results: {results:?}");
  assert_eq!(results[0].path, PathBuf::from("src/main.rs"));
  assert!(results[0].snippet.contains("needle"));
}

#[test]
fn reconciliation_updates_changed_files_and_removes_deleted_files() {
  let workspace = TempWorkspace::new();
  workspace.write("src/lib.rs", "fn old_symbol() {}");
  let index = open_index(&workspace);

  workspace.write("src/lib.rs", "fn new_symbol() {}");
  workspace.write("src/extra.rs", "fn extra_symbol() {}");
  index.reconcile().unwrap();
  fs::remove_file(workspace.0.join("src/lib.rs")).unwrap();
  index.reconcile().unwrap();

  assert!(index.search("old_symbol", 10).unwrap().is_empty());
  assert!(index.search("new_symbol", 10).unwrap().is_empty());
  assert_eq!(index.search("extra_symbol", 10).unwrap().len(), 1);
}

#[test]
fn watcher_reconciles_file_changes_and_recovers_after_debounce() {
  let workspace = TempWorkspace::new();
  workspace.write("src/lib.rs", "fn before_watch() {}");
  let index = open_index(&workspace);
  let _watcher = index.watch().unwrap();

  workspace.write("src/lib.rs", "fn after_watch() {}");

  assert!(wait_for(|| index
    .search("after_watch", 10)
    .is_ok_and(|hits| !hits.is_empty())));
  assert!(index.search("before_watch", 10).unwrap().is_empty());
}

#[test]
fn watcher_removes_deleted_files_from_search_results() {
  let workspace = TempWorkspace::new();
  workspace.write("src/lib.rs", "fn removable_symbol() {}");
  let index = open_index(&workspace);
  let _watcher = index.watch().unwrap();

  fs::remove_file(workspace.0.join("src/lib.rs")).unwrap();

  assert!(wait_for(|| index
    .search("removable_symbol", 10)
    .is_ok_and(|hits| hits.is_empty())));
}

#[test]
fn watcher_reapplies_updated_gitignore_rules() {
  let workspace = TempWorkspace::new();
  workspace.write("src/private.rs", "fn newly_ignored_symbol() {}");
  let index = open_index(&workspace);
  let _watcher = index.watch().unwrap();

  workspace.write(".gitignore", "src/private.rs\n");

  assert!(wait_for(|| index
    .search("newly_ignored_symbol", 10)
    .is_ok_and(|hits| hits.is_empty())));
}

#[test]
fn search_limit_is_bounded_and_zero_limit_returns_nothing() {
  let workspace = TempWorkspace::new();
  for name in ["a.rs", "b.rs", "c.rs"] {
    workspace.write(name, "fn shared_term() {}");
  }
  let index = open_index(&workspace);

  assert!(index.search("shared_term", 0).unwrap().is_empty());
  assert_eq!(index.search("shared_term", 2).unwrap().len(), 2);
}

#[test]
fn search_treats_fts_operators_as_literal_terms() {
  let workspace = TempWorkspace::new();
  workspace.write("src/lib.rs", "fn normal_term() {}");
  let index = open_index(&workspace);

  assert!(index.search("normal_term OR *:*", 10).is_ok());
}

#[test]
fn search_snippets_are_capped_even_for_unusually_long_tokens() {
  let workspace = TempWorkspace::new();
  workspace.write("src/lib.rs", &format!("needle {}", "x".repeat(2_000)));
  let index = open_index(&workspace);

  let results = index.search("needle", 10).unwrap();

  assert_eq!(results.len(), 1);
  assert!(results[0].snippet.chars().count() <= 1_201);
}

fn wait_for(mut predicate: impl FnMut() -> bool) -> bool {
  let deadline = std::time::Instant::now() + Duration::from_secs(3);
  while std::time::Instant::now() < deadline {
    if predicate() {
      return true;
    }
    std::thread::sleep(Duration::from_millis(50));
  }
  false
}
