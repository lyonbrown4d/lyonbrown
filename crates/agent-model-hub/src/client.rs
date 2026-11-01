use std::{
  path::{Path, PathBuf},
  time::Duration,
};

use anyhow::{Result, ensure};
use futures::StreamExt;
use hf_hub::{
  HFClient,
  repository::{ModelInfo, RepoTreeEntry},
};
use tokio_util::sync::CancellationToken;

use crate::{ArtifactFile, HubModel};

#[derive(Clone)]
pub struct HubClient {
  pub(crate) client: HFClient,
  pub(crate) root: PathBuf,
  pub(crate) metadata: reqwest::Client,
  pub(crate) endpoint: url::Url,
}

impl HubClient {
  /// Tokens belong to the host configuration and never enter model metadata.
  pub fn new(endpoint: &str, token: Option<&str>, root: impl AsRef<Path>) -> Result<Self> {
    let url = url::Url::parse(endpoint)?;
    ensure!(
      url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none(),
      "invalid Hub endpoint"
    );
    let loopback = url.host_str().is_some_and(|host| {
      host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
    });
    ensure!(
      url.scheme() == "https" || (url.scheme() == "http" && loopback),
      "Hub endpoint requires HTTPS or literal loopback"
    );
    std::fs::create_dir_all(root.as_ref())?;
    ensure!(
      !std::fs::symlink_metadata(root.as_ref())?
        .file_type()
        .is_symlink(),
      "Hub root must not be a symlink"
    );
    let root = std::fs::canonicalize(root)?;
    let client = HFClient::builder()
      .endpoint(endpoint.trim_end_matches('/'))
      .token(token.unwrap_or(""))
      .cache_dir(root.join("cache"))
      .cache_enabled(false)
      .retry_max_attempts(1)
      .build()?;
    let metadata = crate::metadata::client(token)?;
    Ok(Self {
      client,
      root,
      metadata,
      endpoint: url,
    })
  }

  pub async fn search(
    &self,
    query: &str,
    limit: usize,
    cancellation: &CancellationToken,
  ) -> Result<Vec<HubModel>> {
    ensure!(
      query.len() <= 256 && (1..=100).contains(&limit),
      "invalid Hub search bounds"
    );
    let stream = self
      .client
      .list_models()
      .search(query)
      .limit(limit)
      .send()?;
    futures::pin_mut!(stream);
    let future = async {
      let mut models = Vec::new();
      while let Some(item) = stream.next().await {
        models.push(project(item?));
      }
      Ok(models)
    };
    tokio::select! { biased; _ = cancellation.cancelled() => anyhow::bail!("model search cancelled"),
    result = tokio::time::timeout(Duration::from_secs(30), future) => result?, }
  }

  pub async fn inspect(
    &self,
    repository: &str,
    revision: &str,
    cancellation: &CancellationToken,
  ) -> Result<HubModel> {
    crate::plan::validate_repository(repository)?;
    ensure!(
      !revision.is_empty() && revision.len() <= 128 && !revision.chars().any(char::is_control),
      "invalid revision"
    );
    let (owner, name) = hf_hub::split_id(repository);
    let repo = self.client.model(owner, name);
    let future = async {
      let mut result = project(repo.info().revision(revision).send().await?);
      crate::plan::validate_revision(&result.revision)?;
      let stream = repo
        .list_tree()
        .revision(result.revision.clone())
        .recursive(true)
        .expand(true)
        .limit(2049)
        .send()?;
      futures::pin_mut!(stream);
      let mut count = 0;
      let mut missing_hashes = Vec::new();
      while let Some(entry) = stream.next().await {
        count += 1;
        ensure!(count <= 2048, "model repository has too many entries");
        if let RepoTreeEntry::File {
          path, size, lfs, ..
        } = entry?
        {
          crate::plan::validate_path(&path)?;
          let is_lfs = lfs.is_some();
          let sha256 = lfs.and_then(|value| value.sha256);
          // The Hub tree wire shape uses lfs.oid; older SDK metadata omits it.
          // Preserve the authenticated digest on the Hub's CDN redirect.
          if is_lfs && sha256.is_none() {
            missing_hashes.push((result.files.len(), path.clone(), size));
          }
          result.files.push(ArtifactFile {
            path,
            bytes: size,
            sha256,
          });
        }
      }
      {
        let revision = &result.revision;
        let hashes = futures::stream::iter(missing_hashes)
          .map(|(index, path, size)| async move {
            Ok::<_, anyhow::Error>((
              index,
              self
                .artifact_hash(repository, revision, &path, size)
                .await?,
            ))
          })
          .buffer_unordered(6);
        futures::pin_mut!(hashes);
        while let Some(hash) = hashes.next().await {
          let (index, hash) = hash?;
          result.files[index].sha256 = Some(hash);
        }
      }
      Ok(result)
    };
    tokio::select! { biased; _ = cancellation.cancelled() => anyhow::bail!("model inspection cancelled"),
    result = tokio::time::timeout(Duration::from_secs(30), future) => result?, }
  }
}

fn project(info: ModelInfo) -> HubModel {
  HubModel {
    repository: info.id,
    revision: info.sha.unwrap_or_default(),
    license: info
      .card_data
      .as_ref()
      .and_then(|value| value.get("license"))
      .and_then(|value| value.as_str())
      .map(str::to_owned),
    gated: info
      .gated
      .is_some_and(|value| value != serde_json::Value::Bool(false)),
    architecture: info
      .config
      .as_ref()
      .and_then(|value| value.get("model_type"))
      .and_then(|value| value.as_str())
      .map(str::to_owned),
    files: Vec::new(),
  }
}
