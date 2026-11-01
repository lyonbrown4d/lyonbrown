//! Agentd-owned Skill installation. Clients only select sources and render results.
use std::{io::Cursor, path::PathBuf};

use agent_protocol::v1::{
  self,
  skill_service_server::{SkillService, SkillServiceServer},
};
use agent_security::Authenticate;
use agent_skills::{
  ArchiveFormat, Limits, PreparedSkill, SkillInspection, SkillManager, SkillSource,
};
use futures::StreamExt;
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};

use crate::Daemon;

pub fn skill_service(
  daemon: Daemon,
) -> InterceptedService<SkillServiceServer<Daemon>, Authenticate> {
  let authentication = Authenticate::new(daemon.auth_token());
  SkillServiceServer::with_interceptor(daemon, authentication)
}

#[tonic::async_trait]
impl SkillService for Daemon {
  async fn list_skills(
    &self,
    request: Request<v1::ListSkillsRequest>,
  ) -> Result<Response<v1::Skills>, Status> {
    let input = request.into_inner();
    let manager = self.skill_manager(&input.scope, &input.session_id)?;
    let installed = tokio::task::spawn_blocking(move || manager.list())
      .await
      .map_err(|_| Status::internal("Skill inventory task failed"))?
      .map_err(skill_error)?;
    Ok(Response::new(v1::Skills {
      skills: installed
        .iter()
        .map(|skill| skill_view(&skill.inspection, &input.scope))
        .collect(),
    }))
  }

  async fn inspect_skill(
    &self,
    request: Request<v1::InspectSkillRequest>,
  ) -> Result<Response<v1::SkillInspection>, Status> {
    let input = request.into_inner();
    let manager = self.skill_manager(&input.scope, &input.session_id)?;
    let prepared = prepare_source(manager, &input.source).await?;
    Ok(Response::new(inspection_view(
      prepared.inspection(),
      &input.scope,
    )))
  }

  async fn install_skill(
    &self,
    request: Request<v1::InstallSkillRequest>,
  ) -> Result<Response<v1::Skill>, Status> {
    let input = request.into_inner();
    if input.expected_sha256.len() != 64 {
      return Err(Status::invalid_argument("expected Skill hash is required"));
    }
    let manager = self.skill_manager(&input.scope, &input.session_id)?;
    let prepared = prepare_source(manager, &input.source).await?;
    if prepared.inspection().content_sha256 != input.expected_sha256 {
      return Err(Status::failed_precondition(
        "Skill content changed after inspection",
      ));
    }
    let scope = input.scope.clone();
    let manager = self.skill_manager(&input.scope, &input.session_id)?;
    let installed = tokio::task::spawn_blocking(move || manager.activate(prepared))
      .await
      .map_err(|_| Status::internal("Skill activation task failed"))?
      .map_err(skill_error)?;
    Ok(Response::new(skill_view(&installed.inspection, &scope)))
  }

  async fn remove_skill(
    &self,
    request: Request<v1::RemoveSkillRequest>,
  ) -> Result<Response<v1::Skill>, Status> {
    let input = request.into_inner();
    let manager = self.skill_manager(&input.scope, &input.session_id)?;
    let expected = input.expected_sha256.clone();
    let name = input.name.clone();
    let removed = tokio::task::spawn_blocking(move || manager.quarantine(&name, &expected))
      .await
      .map_err(|_| Status::internal("Skill removal task failed"))?
      .map_err(skill_error)?;
    Ok(Response::new(skill_view(&removed.inspection, &input.scope)))
  }
}

impl Daemon {
  fn skill_manager(&self, scope: &str, session_id: &str) -> Result<SkillManager, Status> {
    let root = match scope {
      "daemon_user" => self
        .config
        .data_path()
        .map_err(|_| Status::failed_precondition("daemon data directory unavailable"))?
        .join("skills"),
      "workspace" => {
        let session = self
          .store
          .session(session_id)
          .map_err(crate::service::db_error)?
          .ok_or_else(|| Status::not_found("session not found"))?;
        PathBuf::from(session.workspace)
          .join(".lyonbrown")
          .join("skills")
      }
      _ => {
        return Err(Status::invalid_argument(
          "unsupported Skill installation scope",
        ));
      }
    };
    SkillManager::open(root, Limits::default()).map_err(skill_error)
  }
}

async fn prepare_source(manager: SkillManager, source: &str) -> Result<PreparedSkill, Status> {
  match SkillSource::parse(source).map_err(skill_error)? {
    SkillSource::Local { path } => {
      tokio::task::spawn_blocking(move || manager.prepare_directory(path))
        .await
        .map_err(|_| Status::internal("Skill inspection task failed"))?
        .map_err(skill_error)
    }
    source @ (SkillSource::Https { .. } | SkillSource::GitHub { .. }) => {
      let (url, format, directory) = remote_source(&source)?;
      let bytes = download(&url, Limits::default().download_bytes).await?;
      tokio::task::spawn_blocking(move || {
        manager.prepare_archive(Cursor::new(bytes), format, directory.as_deref())
      })
      .await
      .map_err(|_| Status::internal("Skill archive task failed"))?
      .map_err(skill_error)
    }
  }
}

fn remote_source(source: &SkillSource) -> Result<(String, ArchiveFormat, Option<String>), Status> {
  match source {
    SkillSource::Https { url } => {
      let format = archive_format(url.path())?;
      Ok((url.to_string(), format, None))
    }
    SkillSource::GitHub {
      owner,
      repository,
      revision,
      directory,
    } => {
      let revision = revision
        .as_deref()
        .ok_or_else(|| Status::invalid_argument("GitHub Skill source must pin a revision"))?;
      let root = format!("{repository}-{revision}");
      let directory = directory.as_ref().map(|path| format!("{root}/{path}"));
      Ok((
        format!("https://codeload.github.com/{owner}/{repository}/tar.gz/{revision}"),
        ArchiveFormat::TarGz,
        directory,
      ))
    }
    SkillSource::Local { .. } => Err(Status::internal("local source routed as remote")),
  }
}

fn archive_format(path: &str) -> Result<ArchiveFormat, Status> {
  if path.ends_with(".zip") {
    Ok(ArchiveFormat::Zip)
  } else if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
    Ok(ArchiveFormat::TarGz)
  } else if path.ends_with(".tar") {
    Ok(ArchiveFormat::Tar)
  } else {
    Err(Status::invalid_argument(
      "remote Skill source must be a supported archive",
    ))
  }
}

async fn download(url: &str, limit: u64) -> Result<Vec<u8>, Status> {
  let response = reqwest::Client::builder()
    .redirect(reqwest::redirect::Policy::limited(3))
    .timeout(std::time::Duration::from_secs(30))
    .build()
    .map_err(|_| Status::internal("Skill downloader unavailable"))?
    .get(url)
    .send()
    .await
    .map_err(|_| Status::unavailable("Skill download failed"))?
    .error_for_status()
    .map_err(|_| Status::failed_precondition("Skill source returned an error"))?;
  if response
    .content_length()
    .is_some_and(|length| length > limit)
  {
    return Err(Status::resource_exhausted("Skill download exceeds limit"));
  }
  let mut stream = response.bytes_stream();
  let mut bytes = Vec::new();
  while let Some(chunk) = stream.next().await {
    let chunk = chunk.map_err(|_| Status::unavailable("Skill download interrupted"))?;
    if bytes.len().saturating_add(chunk.len()) > limit as usize {
      return Err(Status::resource_exhausted("Skill download exceeds limit"));
    }
    bytes.extend_from_slice(&chunk);
  }
  Ok(bytes)
}

fn skill_view(inspection: &SkillInspection, scope: &str) -> v1::Skill {
  v1::Skill {
    name: inspection.manifest.name.clone(),
    description: inspection.manifest.description.clone(),
    compatibility: inspection
      .manifest
      .compatibility
      .clone()
      .unwrap_or_default(),
    content_sha256: inspection.content_sha256.clone(),
    total_bytes: inspection.total_bytes,
    files: inspection.files.len() as u32,
    scope: scope.into(),
    enabled: true,
  }
}

fn inspection_view(inspection: &SkillInspection, scope: &str) -> v1::SkillInspection {
  v1::SkillInspection {
    skill: Some(skill_view(inspection, scope)),
    files: inspection
      .files
      .iter()
      .map(|file| v1::SkillFile {
        path: file.path.clone(),
        bytes: file.bytes,
        sha256: file.sha256.clone(),
      })
      .collect(),
  }
}

fn skill_error(_: agent_skills::SkillError) -> Status {
  Status::failed_precondition("Skill source or installation is invalid")
}
