use std::{
  fs,
  io::{Cursor, Write},
  path::{Path, PathBuf},
};

use lyonbrown_skills::{ArchiveFormat, Limits, SkillError, SkillManager};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn skill(parent: &Path, name: &str) -> Result<PathBuf, std::io::Error> {
  let directory = parent.join(name);
  fs::create_dir(&directory)?;
  fs::write(
    directory.join("SKILL.md"),
    format!(
      "---\nname: {name}\ndescription: Review Rust changes.\ncompatibility: Requires cargo\nmetadata:\n  version: '1.0'\nallowed-tools: 'Bash(*)'\n---\nRead [guide](references/guide.md).\n"
    ),
  )?;
  fs::create_dir(directory.join("references"))?;
  fs::write(
    directory.join("references/guide.md"),
    "Inspect code and report findings.\n",
  )?;
  Ok(directory)
}

fn zip_files(files: &[(&str, &[u8])]) -> Result<Cursor<Vec<u8>>, zip::result::ZipError> {
  let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
  for (path, content) in files {
    writer.start_file(
      *path,
      zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )?;
    writer.write_all(content)?;
  }
  let mut reader = writer.finish()?;
  reader.set_position(0);
  Ok(reader)
}

#[test]
fn prepares_then_activates_and_removes_only_the_inspected_version() -> TestResult {
  let source = tempfile::tempdir()?;
  let storage = tempfile::tempdir()?;
  let source = skill(source.path(), "rust-review")?;
  let manager = SkillManager::open(storage.path(), Limits::default())?;
  let prepared = manager.prepare_directory(&source)?;
  assert!(
    manager.list()?.is_empty(),
    "preparation must not activate the Skill"
  );
  assert_eq!(prepared.inspection().files.len(), 2);
  assert_eq!(
    prepared.inspection().manifest.allowed_tools.as_deref(),
    Some("Bash(*)")
  );
  let activated = manager.activate(prepared)?;
  assert_eq!(manager.list()?.len(), 1);
  assert_eq!(activated.inspection, manager.inspect_directory(&source)?);
  assert!(matches!(
    manager.activate(manager.prepare_directory(&source)?),
    Err(SkillError::AlreadyInstalled)
  ));
  assert!(matches!(
    manager.quarantine("rust-review", "wrong-hash"),
    Err(SkillError::ContentChanged)
  ));
  assert!(activated.path.exists());
  let removed = manager.quarantine("rust-review", &activated.inspection.content_sha256)?;
  assert!(manager.list()?.is_empty());
  assert!(
    removed.path.join("SKILL.md").exists(),
    "removal must be recoverable"
  );
  Ok(())
}

#[test]
fn discarded_preparation_cleans_staging_and_never_executes_scripts() -> TestResult {
  let source = tempfile::tempdir()?;
  let storage = tempfile::tempdir()?;
  let source = skill(source.path(), "rust-review")?;
  fs::create_dir(source.join("scripts"))?;
  fs::write(source.join("scripts/install.sh"), "exit 99\n")?;
  let manager = SkillManager::open(storage.path(), Limits::default())?;
  let prepared = manager.prepare_directory(source)?;
  let staging = prepared.staged_path().to_path_buf();
  assert!(staging.exists());
  drop(prepared);
  assert!(!staging.exists());
  assert!(manager.list()?.is_empty());
  Ok(())
}

#[test]
fn rejects_directory_name_mismatch_and_escaping_markdown_references() -> TestResult {
  let source = tempfile::tempdir()?;
  let storage = tempfile::tempdir()?;
  let path = skill(source.path(), "rust-review")?;
  let manager = SkillManager::open(storage.path(), Limits::default())?;
  fs::rename(&path, source.path().join("different"))?;
  assert!(matches!(
    manager.prepare_directory(source.path().join("different")),
    Err(SkillError::Manifest(_))
  ));
  fs::rename(source.path().join("different"), &path)?;
  fs::write(
    path.join("references/guide.md"),
    "[secret](../../secret.txt)",
  )?;
  assert!(matches!(
    manager.prepare_directory(&path),
    Err(SkillError::UnsafePath)
  ));
  fs::write(path.join("references/guide.md"), "[parent](../SKILL.md)")?;
  assert!(manager.prepare_directory(&path).is_ok());
  fs::write(
    path.join("references/guide.md"),
    "[secret](%2e%2e/%2e%2e/secret.txt)",
  )?;
  assert!(matches!(
    manager.prepare_directory(&path),
    Err(SkillError::UnsafePath)
  ));
  Ok(())
}

#[test]
fn zip_installation_selects_one_skill_and_hashes_contents() -> TestResult {
  let storage = tempfile::tempdir()?;
  let manager = SkillManager::open(storage.path(), Limits::default())?;
  let manifest = b"---\nname: rust-review\ndescription: Review changes\n---\n";
  let archive = zip_files(&[
    ("pack/rust-review/SKILL.md", manifest),
    ("pack/rust-review/assets/test.txt", b"fixture"),
  ])?;
  let prepared = manager.prepare_archive(archive, ArchiveFormat::Zip, Some("pack/rust-review"))?;
  assert_eq!(prepared.inspection().files.len(), 2);
  let hash = prepared.inspection().content_sha256.clone();
  manager.activate(prepared)?;
  assert_eq!(manager.list()?[0].inspection.content_sha256, hash);
  let flat = zip_files(&[("SKILL.md", manifest)])?;
  assert_eq!(
    manager
      .prepare_archive(flat, ArchiveFormat::Zip, None)?
      .inspection()
      .manifest
      .name,
    "rust-review"
  );
  let document = manager.prepare_document(std::str::from_utf8(manifest)?)?;
  assert_eq!(document.inspection().files.len(), 1);
  Ok(())
}

#[test]
fn unsafe_archives_leave_no_activated_or_staged_files() -> TestResult {
  let storage = tempfile::tempdir()?;
  let manager = SkillManager::open(storage.path(), Limits::default())?;
  for path in [
    "../escape.txt",
    "/escape.txt",
    "folder\\escape.txt",
    "CON",
    "NUL.txt",
    "folder/../escape.txt",
  ] {
    let archive = zip_files(&[(path, b"unsafe")])?;
    assert!(
      manager
        .prepare_archive(archive, ArchiveFormat::Zip, None)
        .is_err()
    );
  }
  let archive = zip_files(&[("A.txt", b"one"), ("a.txt", b"two")])?;
  assert!(matches!(
    manager.prepare_archive(archive, ArchiveFormat::Zip, None),
    Err(SkillError::UnsafePath)
  ));
  assert!(manager.list()?.is_empty());
  let names = fs::read_dir(storage.path())?
    .map(|entry| entry.map(|entry| entry.file_name()))
    .collect::<Result<Vec<_>, _>>()?;
  assert!(
    names
      .iter()
      .all(|name| !name.to_string_lossy().starts_with(".staging-"))
  );
  Ok(())
}

#[test]
fn tar_rejects_links_and_supports_regular_files() -> TestResult {
  let storage = tempfile::tempdir()?;
  let manager = SkillManager::open(storage.path(), Limits::default())?;
  let manifest = b"---\nname: rust-review\ndescription: Review changes\n---\n";
  let mut builder = tar::Builder::new(Vec::new());
  let mut header = tar::Header::new_gnu();
  header.set_size(manifest.len() as u64);
  header.set_mode(0o644);
  header.set_cksum();
  builder.append_data(&mut header, "rust-review/SKILL.md", &manifest[..])?;
  let data = builder.into_inner()?;
  let archive = Cursor::new(data.clone());
  assert!(
    manager
      .prepare_archive(archive, ArchiveFormat::Tar, None)
      .is_ok()
  );
  let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
  gzip.write_all(&data)?;
  assert!(
    manager
      .prepare_archive(Cursor::new(gzip.finish()?), ArchiveFormat::TarGz, None)
      .is_ok()
  );
  let mut builder = tar::Builder::new(Vec::new());
  let mut header = tar::Header::new_gnu();
  header.set_entry_type(tar::EntryType::Symlink);
  header.set_size(0);
  header.set_mode(0o777);
  builder.append_link(&mut header, "rust-review/secret", "../../secret")?;
  assert!(matches!(
    manager.prepare_archive(Cursor::new(builder.into_inner()?), ArchiveFormat::Tar, None),
    Err(SkillError::UnsupportedEntry)
  ));
  Ok(())
}

#[test]
fn byte_entry_depth_and_download_limits_fail_before_activation() -> TestResult {
  let storage = tempfile::tempdir()?;
  let manifest = b"---\nname: rust-review\ndescription: Review changes\n---\n";
  for limits in [
    Limits {
      download_bytes: 1,
      ..Limits::default()
    },
    Limits {
      extracted_bytes: 1,
      ..Limits::default()
    },
    Limits {
      file_bytes: 1,
      ..Limits::default()
    },
    Limits {
      files: 0,
      ..Limits::default()
    },
    Limits {
      depth: 1,
      ..Limits::default()
    },
  ] {
    let manager = SkillManager::open(storage.path(), limits)?;
    let archive = zip_files(&[("rust-review/SKILL.md", manifest)])?;
    assert!(matches!(
      manager.prepare_archive(archive, ArchiveFormat::Zip, None),
      Err(SkillError::Limit(_))
    ));
    assert!(manager.list()?.is_empty());
  }
  Ok(())
}

#[cfg(unix)]
#[test]
fn local_symlinks_are_rejected() -> TestResult {
  let source = tempfile::tempdir()?;
  let storage = tempfile::tempdir()?;
  let path = skill(source.path(), "rust-review")?;
  std::os::unix::fs::symlink("/etc/passwd", path.join("secret"))?;
  let manager = SkillManager::open(storage.path(), Limits::default())?;
  assert!(matches!(
    manager.prepare_directory(path),
    Err(SkillError::UnsupportedEntry)
  ));
  Ok(())
}

#[test]
fn activation_rechecks_staged_content() -> TestResult {
  let source = tempfile::tempdir()?;
  let storage = tempfile::tempdir()?;
  let path = skill(source.path(), "rust-review")?;
  let manager = SkillManager::open(storage.path(), Limits::default())?;
  let prepared = manager.prepare_directory(path)?;
  fs::write(
    prepared.staged_path().join("references/guide.md"),
    "changed",
  )?;
  assert!(matches!(
    manager.activate(prepared),
    Err(SkillError::ContentChanged)
  ));
  assert!(manager.list()?.is_empty());
  Ok(())
}
