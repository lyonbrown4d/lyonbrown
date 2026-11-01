use agent_workspace::{
  Workspace, load_repository_instructions, load_repository_instructions_for_paths,
};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn open_workspace(root: &TempDir) -> Workspace {
  Workspace::open(root.path()).expect("open test workspace")
}

#[test]
fn missing_root_instructions_return_empty() {
  let root = TempDir::new().expect("create workspace");
  let instructions = load_repository_instructions(&open_workspace(&root))
    .expect("load missing repository instructions");
  assert!(instructions.is_empty());
}

#[test]
fn root_and_relevant_nested_instructions_are_returned_in_scope_order() {
  let root = TempDir::new().expect("create workspace");
  fs::write(root.path().join("AGENTS.md"), "root guidance").expect("write root instructions");
  fs::create_dir_all(root.path().join("src/nested")).expect("create nested directories");
  fs::write(root.path().join("src/AGENTS.md"), "source guidance")
    .expect("write source instructions");
  fs::write(root.path().join("src/nested/AGENTS.md"), "nested guidance")
    .expect("write nested instructions");
  fs::write(root.path().join("src/nested/main.rs"), "fn main() {}").expect("write nested file");
  fs::create_dir(root.path().join("docs")).expect("create unrelated directory");
  fs::write(root.path().join("docs/AGENTS.md"), "unrelated guidance")
    .expect("write unrelated instructions");

  let instructions = load_repository_instructions_for_paths(
    &open_workspace(&root),
    &[PathBuf::from("src/nested/main.rs")],
  )
  .expect("load relevant instructions");
  assert_eq!(
    instructions
      .iter()
      .map(|item| (item.relative_path.as_str(), item.content.as_str()))
      .collect::<Vec<_>>(),
    vec![
      ("AGENTS.md", "root guidance"),
      ("src/AGENTS.md", "source guidance"),
      ("src/nested/AGENTS.md", "nested guidance"),
    ]
  );
}

#[test]
fn missing_nested_path_keeps_existing_ancestor_instructions() {
  let root = TempDir::new().expect("create workspace");
  fs::create_dir(root.path().join("src")).expect("create source directory");
  fs::write(root.path().join("src/AGENTS.md"), "source guidance")
    .expect("write source instructions");

  let instructions = load_repository_instructions_for_paths(
    &open_workspace(&root),
    &[PathBuf::from("src/not-created/file.rs")],
  )
  .expect("load ancestor instructions for missing path");
  assert_eq!(instructions.len(), 1);
  assert_eq!(instructions[0].relative_path, "src/AGENTS.md");
}

#[cfg(unix)]
#[test]
fn unix_backslash_in_directory_name_is_preserved() {
  let root = TempDir::new().expect("create workspace");
  let directory = root.path().join("sub\\dir");
  fs::create_dir(&directory).expect("create directory with backslash");
  fs::write(directory.join("AGENTS.md"), "scoped guidance").expect("write scoped instructions");

  let instructions = load_repository_instructions_for_paths(
    &open_workspace(&root),
    &[PathBuf::from("sub\\dir/file.rs")],
  )
  .expect("load instructions from directory with backslash");
  assert_eq!(instructions.len(), 1);
  assert_eq!(instructions[0].relative_path, "sub\\dir/AGENTS.md");
}

#[test]
fn oversized_instructions_fail_explicitly() {
  let root = TempDir::new().expect("create workspace");
  fs::write(root.path().join("AGENTS.md"), vec![b'x'; 32 * 1024 + 1])
    .expect("write oversized instructions");

  let error = load_repository_instructions(&open_workspace(&root))
    .expect_err("oversized repository instructions must fail");
  assert!(
    error.to_string().contains("32 KiB-byte limit")
      || error.to_string().contains("32768-byte limit")
  );
}

#[test]
fn aggregate_instruction_limit_fails_explicitly() {
  let root = TempDir::new().expect("create workspace");
  fs::write(root.path().join("AGENTS.md"), vec![b'r'; 24 * 1024]).expect("write root instructions");
  fs::create_dir_all(root.path().join("src/nested")).expect("create nested directories");
  fs::write(root.path().join("src/AGENTS.md"), vec![b's'; 24 * 1024])
    .expect("write source instructions");
  fs::write(
    root.path().join("src/nested/AGENTS.md"),
    vec![b'n'; 24 * 1024],
  )
  .expect("write nested instructions");

  let error = load_repository_instructions_for_paths(
    &open_workspace(&root),
    &[PathBuf::from("src/nested/file.rs")],
  )
  .expect_err("aggregate instruction size must be bounded");
  assert!(error.to_string().contains("aggregate limit"));
}

#[test]
fn context_path_cannot_escape_the_workspace() {
  let root = TempDir::new().expect("create workspace");
  let error = load_repository_instructions_for_paths(
    &open_workspace(&root),
    &[PathBuf::from("../outside/file.rs")],
  )
  .expect_err("parent traversal must be rejected");
  assert!(
    error
      .to_string()
      .contains("stay within the authorized workspace")
  );
}

#[test]
fn invalid_utf8_instructions_fail_explicitly() {
  let root = TempDir::new().expect("create workspace");
  fs::write(root.path().join("AGENTS.md"), [0xff, 0xfe]).expect("write invalid UTF-8");

  let error = load_repository_instructions(&open_workspace(&root))
    .expect_err("invalid UTF-8 repository instructions must fail");
  assert!(error.to_string().contains("read repository instructions"));
}

#[cfg(unix)]
#[test]
fn outside_symlink_is_not_followed_for_instruction_context() {
  use std::os::unix::fs::symlink;

  let root = TempDir::new().expect("create workspace");
  let outside = TempDir::new().expect("create outside directory");
  fs::write(outside.path().join("AGENTS.md"), "outside secret").expect("write outside file");
  symlink(outside.path(), root.path().join("external")).expect("create outside symlink");

  let error = load_repository_instructions_for_paths(
    &open_workspace(&root),
    &[PathBuf::from("external/file.rs")],
  )
  .expect_err("symlink context should fail explicitly without escaping the workspace");
  assert!(
    error
      .to_string()
      .contains("symlinked context paths are unsupported")
  );
}

#[cfg(unix)]
#[test]
fn symlinked_agents_file_fails_explicitly() {
  use std::os::unix::fs::symlink;

  let root = TempDir::new().expect("create workspace");
  let outside = TempDir::new().expect("create outside directory");
  let external_instructions = outside.path().join("AGENTS.md");
  fs::write(&external_instructions, "outside guidance").expect("write outside instructions");
  symlink(external_instructions, root.path().join("AGENTS.md"))
    .expect("create instructions symlink");

  let error = load_repository_instructions(&open_workspace(&root))
    .expect_err("symlinked instructions must fail explicitly");
  assert!(
    error
      .to_string()
      .contains("symlinked instructions are unsupported")
  );
}
