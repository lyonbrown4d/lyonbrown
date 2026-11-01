use std::{
  fs,
  io::{Read, Seek},
  path::{Path, PathBuf},
};

use fs2::FileExt;
use tempfile::TempDir;

use crate::{
  ArchiveFormat, Limits, Result, SkillError, SkillInspection, archive, inventory,
  manifest::validate_name, parse_manifest, paths,
};

/// Reusable file engine. Call only after recording an authorized operation intent.
pub struct SkillManager {
  root: PathBuf,
  limits: Limits,
}

/// Private staging is deleted when dropped, including validation failures.
pub struct PreparedSkill {
  staging: TempDir,
  content: PathBuf,
  inspection: SkillInspection,
}

impl PreparedSkill {
  pub fn inspection(&self) -> &SkillInspection {
    &self.inspection
  }

  /// For an authorized inspection UI; the caller must treat these files as data.
  pub fn staged_path(&self) -> &Path {
    &self.content
  }
}

#[derive(Clone, Debug)]
pub struct ActivatedSkill {
  pub path: PathBuf,
  pub inspection: SkillInspection,
}

/// Recovery location for an explicit removal or update; no files are deleted.
#[derive(Clone, Debug)]
pub struct QuarantinedSkill {
  pub path: PathBuf,
  pub inspection: SkillInspection,
}

impl SkillManager {
  pub fn open(root: impl AsRef<Path>, limits: Limits) -> Result<Self> {
    fs::create_dir_all(root.as_ref())?;
    if !fs::symlink_metadata(root.as_ref())?.is_dir() {
      return Err(SkillError::UnsupportedEntry);
    }
    Ok(Self {
      root: root.as_ref().canonicalize()?,
      limits,
    })
  }

  pub fn root(&self) -> &Path {
    &self.root
  }

  pub fn inspect_directory(&self, path: impl AsRef<Path>) -> Result<SkillInspection> {
    inventory::inspect(path.as_ref(), self.limits)
  }

  pub fn prepare_directory(&self, source: impl AsRef<Path>) -> Result<PreparedSkill> {
    let inspection = self.inspect_directory(&source)?;
    let staging = self.staging()?;
    let content = staging.path().join(&inspection.manifest.name);
    inventory::copy_directory(source.as_ref(), &content, self.limits)?;
    let copied = inventory::inspect(&content, self.limits)?;
    if copied != inspection {
      return Err(SkillError::ContentChanged);
    }
    Ok(PreparedSkill {
      staging,
      content,
      inspection,
    })
  }

  /// Prepare a direct SKILL.md download. Linked resources are not fetched.
  pub fn prepare_document(&self, content: &str) -> Result<PreparedSkill> {
    if content.len() as u64 > self.limits.download_bytes
      || content.len() as u64 > self.limits.file_bytes
    {
      return Err(SkillError::Limit("download or file bytes"));
    }
    let manifest = parse_manifest(content)?.manifest;
    let staging = self.staging()?;
    let path = staging.path().join(&manifest.name);
    fs::create_dir(&path)?;
    fs::write(path.join("SKILL.md"), content)?;
    let inspection = inventory::inspect(&path, self.limits)?;
    Ok(PreparedSkill {
      staging,
      content: path,
      inspection,
    })
  }

  /// Agentd supplies an already-authorized downloaded file or local archive.
  /// `skill_path` selects one directory inside a multi-Skill repository archive.
  pub fn prepare_archive(
    &self,
    reader: impl Read + Seek,
    format: ArchiveFormat,
    skill_path: Option<&str>,
  ) -> Result<PreparedSkill> {
    let staging = self.staging()?;
    let extracted = staging.path().join(".package");
    fs::create_dir(&extracted)?;
    archive::extract(reader, format, &extracted, self.limits)?;
    let selected = match skill_path {
      Some(path) => extracted.join(paths::relative(path, self.limits.depth)?),
      None => {
        let candidates = discover(&extracted)?;
        if candidates.len() != 1 {
          return Err(SkillError::Manifest(
            "select one Skill directory from the archive".into(),
          ));
        }
        candidates
          .into_iter()
          .next()
          .ok_or_else(|| SkillError::Manifest("no Skill found".into()))?
      }
    };
    let manifest = parse_manifest(&fs::read_to_string(selected.join("SKILL.md"))?)?.manifest;
    // A flat archive has no named root; assign the validated manifest name.
    if selected != extracted
      && selected.file_name().and_then(|name| name.to_str()) != Some(&manifest.name)
    {
      return Err(SkillError::Manifest(
        "directory name differs from Skill name".into(),
      ));
    }
    let content = staging.path().join(&manifest.name);
    fs::rename(selected, &content)?;
    let inspection = inventory::inspect(&content, self.limits)?;
    Ok(PreparedSkill {
      staging,
      content,
      inspection,
    })
  }

  /// Explicit final activation. The caller persists the returned receipt and event.
  /// Existing Skills are never replaced; updates use quarantine + activate under
  /// the daemon's durable operation and session-version binding policy.
  pub fn activate(&self, prepared: PreparedSkill) -> Result<ActivatedSkill> {
    if prepared.staging.path().parent() != Some(self.root.as_path()) {
      return Err(SkillError::UnsafePath);
    }
    let _lock = self.lock()?;
    let inspection = inventory::inspect(&prepared.content, self.limits)?;
    if inspection != prepared.inspection {
      return Err(SkillError::ContentChanged);
    }
    let target = self.root.join(&inspection.manifest.name);
    if fs::symlink_metadata(&target).is_ok() {
      return Err(SkillError::AlreadyInstalled);
    }
    fs::rename(&prepared.content, &target)?;
    Ok(ActivatedSkill {
      path: target,
      inspection,
    })
  }

  pub fn list(&self) -> Result<Vec<ActivatedSkill>> {
    let _lock = self.lock()?;
    let mut installed = Vec::new();
    for entry in fs::read_dir(&self.root)? {
      let path = entry?.path();
      let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(SkillError::UnsafePath)?;
      if name.starts_with('.') {
        continue;
      }
      validate_name(name)?;
      installed.push(ActivatedSkill {
        inspection: inventory::inspect(&path, self.limits)?,
        path,
      });
    }
    installed.sort_by(|left, right| {
      left
        .inspection
        .manifest
        .name
        .cmp(&right.inspection.manifest.name)
    });
    Ok(installed)
  }

  /// Reversible removal guarded by the exact content hash the user inspected.
  pub fn quarantine(&self, name: &str, expected_sha256: &str) -> Result<QuarantinedSkill> {
    validate_name(name)?;
    let _lock = self.lock()?;
    let source = self.root.join(name);
    if !source.try_exists()? {
      return Err(SkillError::NotInstalled);
    }
    let inspection = inventory::inspect(&source, self.limits)?;
    if inspection.content_sha256 != expected_sha256 {
      return Err(SkillError::ContentChanged);
    }
    let quarantine = tempfile::Builder::new()
      .prefix(".quarantine-")
      .tempdir_in(&self.root)?;
    let path = quarantine.path().join(name);
    fs::rename(source, &path)?;
    // Keep the directory for daemon recovery/cleanup after its receipt commits.
    let _ = quarantine.keep();
    Ok(QuarantinedSkill { path, inspection })
  }

  fn staging(&self) -> Result<TempDir> {
    Ok(
      tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(&self.root)?,
    )
  }

  fn lock(&self) -> Result<fs::File> {
    let file = fs::OpenOptions::new()
      .create(true)
      .truncate(false)
      .read(true)
      .write(true)
      .open(self.root.join(".manager.lock"))?;
    file.lock_exclusive()?;
    Ok(file)
  }
}

fn discover(root: &Path) -> Result<Vec<PathBuf>> {
  let mut result = Vec::new();
  if root.join("SKILL.md").is_file() {
    result.push(root.to_path_buf());
  }
  for entry in fs::read_dir(root)? {
    let path = entry?.path();
    if fs::symlink_metadata(&path)?.is_dir() {
      result.extend(discover(&path)?);
    }
  }
  Ok(result)
}
