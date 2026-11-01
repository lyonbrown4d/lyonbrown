use std::fs::File;

use anyhow::{Result, ensure};
use fs2::FileExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::files::{atomic_json, publish, read_installed, subdir, validate_id};
use crate::{HubClient, InstallPlan, InstallProgress, InstallRecord, InstalledModel};

impl HubClient {
  /// A plan is re-inspected before use; hash-known artifacts may resume partial bytes.
  /// The caller owns persistent Task intent and events; this manifest records artifact state.
  pub async fn install(
    &self,
    plan: &InstallPlan,
    cancellation: &CancellationToken,
    progress: Option<mpsc::Sender<InstallProgress>>,
  ) -> Result<InstalledModel> {
    validate_id(&plan.id)?;
    let inspected = self
      .inspect(&plan.repository, &plan.revision, cancellation)
      .await?;
    let selected = if plan.format == "gguf" {
      plan.entrypoint.as_str()
    } else {
      plan
        .files
        .iter()
        .find(|file| file.path.ends_with(".safetensors"))
        .map(|file| file.path.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing weights"))?
    };
    let verified = crate::plan(&inspected, selected, plan.runtime)?;
    ensure!(
      serde_json::to_value(&verified)? == serde_json::to_value(plan)?,
      "installation plan changed"
    );
    let lock_root = subdir(&self.root, "locks")?;
    let lock_path = lock_root.join(&plan.id);
    if let Ok(metadata) = std::fs::symlink_metadata(&lock_path) {
      ensure!(
        !metadata.file_type().is_symlink(),
        "invalid installation lock"
      );
    }
    let lock = File::options()
      .create(true)
      .truncate(false)
      .read(true)
      .write(true)
      .open(lock_path)?;
    lock
      .try_lock_exclusive()
      .map_err(|_| anyhow::anyhow!("model installation is busy"))?;
    let models = subdir(&self.root, "models")?;
    let target = models.join(&plan.id);
    if target.exists() {
      return read_installed(&target);
    }
    let stage = subdir(&subdir(&self.root, "staging")?, &plan.id)?;
    ensure!(
      fs2::available_space(&stage)? >= plan.total_bytes,
      "insufficient disk space for model installation"
    );
    let jobs = subdir(&self.root, "jobs")?;
    let record_path = jobs.join(format!("{}.json", plan.id));
    let mut record = InstallRecord {
      plan: plan.clone(),
      progress: InstallProgress {
        id: plan.id.clone(),
        state: "downloading".into(),
        downloaded_bytes: 0,
        total_bytes: plan.total_bytes,
        current_file: String::new(),
      },
    };
    atomic_json(&record_path, &record)?;
    let result = self
      .transfer(plan, &stage, cancellation, &progress, &mut record)
      .await;
    if let Err(error) = result {
      record.progress.state = if cancellation.is_cancelled() {
        "cancelled"
      } else {
        "failed"
      }
      .into();
      atomic_json(&record_path, &record)?;
      publish(&progress, &record.progress);
      return Err(error);
    }
    ensure!(!cancellation.is_cancelled(), "model installation cancelled");
    let verification_root = stage.clone();
    let artifacts = plan.files.clone();
    let verified_files = tokio::task::spawn_blocking(move || {
      artifacts
        .into_iter()
        .map(|mut artifact| {
          artifact.sha256 = Some(crate::files::digest(
            &verification_root.join(&artifact.path),
          )?);
          Ok(artifact)
        })
        .collect::<Result<Vec<_>>>()
    })
    .await??;
    ensure!(!cancellation.is_cancelled(), "model installation cancelled");
    let installed = InstalledModel {
      plan: plan.clone(),
      verified_files,
      directory: target.clone(),
      entrypoint: if plan.entrypoint == "." {
        target.clone()
      } else {
        target.join(&plan.entrypoint)
      },
      state: "installed_unverified".into(),
    };
    atomic_json(&stage.join("installed.json"), &installed)?;
    record.progress.state = "activating".into();
    atomic_json(&record_path, &record)?;
    std::fs::rename(&stage, &target)?;
    record.progress.state = "installed_unverified".into();
    atomic_json(&record_path, &record)?;
    publish(&progress, &record.progress);
    Ok(installed)
  }

  pub fn installed(&self) -> Result<Vec<InstalledModel>> {
    let root = subdir(&self.root, "models")?;
    let mut result = Vec::new();
    for entry in std::fs::read_dir(root)? {
      let entry = entry?;
      ensure!(
        !entry.file_type()?.is_symlink(),
        "invalid installed model entry"
      );
      if entry.file_type()?.is_dir() {
        result.push(read_installed(&entry.path())?);
      }
    }
    result.sort_by(|left, right| left.plan.id.cmp(&right.plan.id));
    Ok(result)
  }

  pub fn jobs(&self) -> Result<Vec<InstallRecord>> {
    let root = subdir(&self.root, "jobs")?;
    let mut result = Vec::new();
    for entry in std::fs::read_dir(root)? {
      let entry = entry?;
      if entry
        .path()
        .extension()
        .is_some_and(|value| value == "json")
      {
        ensure!(
          !entry.file_type()?.is_symlink() && entry.metadata()?.len() <= 2 * 1024 * 1024,
          "invalid installation record"
        );
        let mut record: InstallRecord = serde_json::from_reader(File::open(entry.path())?)?;
        // An interrupted worker does not imply the artifact was activated.
        if matches!(record.progress.state.as_str(), "downloading" | "activating") {
          validate_id(&record.plan.id)?;
          let lock_path = self.root.join("locks").join(&record.plan.id);
          let worker_active = File::open(lock_path)
            .ok()
            .is_some_and(|lock| lock.try_lock_exclusive().is_err());
          if !worker_active {
            record.progress.state = if self.root.join("models").join(&record.plan.id).is_dir() {
              "installed_unverified"
            } else {
              "interrupted"
            }
            .into();
          }
        }
        result.push(record);
      }
    }
    Ok(result)
  }

  /// Recoverable removal; callers must first reject loaded or referenced artifacts.
  pub fn remove(&self, id: &str) -> Result<()> {
    validate_id(id)?;
    let source = subdir(&self.root, "models")?.join(id);
    ensure!(
      source.is_dir() && !std::fs::symlink_metadata(&source)?.file_type().is_symlink(),
      "installed model not found"
    );
    let quarantine = subdir(&self.root, "quarantine")?;
    std::fs::rename(
      source,
      quarantine.join(format!("{id}-{}", uuid::Uuid::new_v4())),
    )?;
    Ok(())
  }
}
