use crate::Daemon;
use agent_protocol::v1::{
  self,
  workspace_service_server::{WorkspaceService, WorkspaceServiceServer},
};
use agent_security::Authenticate;
use agent_workspace::{Workspace, WorkspaceWatch};
use std::{pin::Pin, sync::Arc, time::Duration};
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};

pub fn workspace_service(
  daemon: Daemon,
) -> InterceptedService<WorkspaceServiceServer<Daemon>, Authenticate> {
  let authentication = Authenticate::new(daemon.auth_token());
  WorkspaceServiceServer::with_interceptor(daemon, authentication)
}
fn failure(_: anyhow::Error) -> Status {
  Status::failed_precondition(
    "workspace operation unavailable, outside authorized scope, or file revision changed",
  )
}
impl Daemon {
  fn workspace_path(&self, session_id: &str) -> Result<String, Status> {
    self
      .store
      .session(session_id)
      .map_err(failure)?
      .map(|session| session.workspace)
      .ok_or_else(|| Status::not_found("session not found"))
  }
}
#[tonic::async_trait]
impl WorkspaceService for Daemon {
  async fn list_files(
    &self,
    request: Request<v1::ListFilesRequest>,
  ) -> Result<Response<v1::WorkspaceFiles>, Status> {
    let input = request.into_inner();
    let path = self.workspace_path(&input.session_id)?;
    let entries =
      tokio::task::spawn_blocking(move || Workspace::open(path)?.list(&input.directory))
        .await
        .map_err(|_| Status::internal("workspace task failed"))?
        .map_err(failure)?;
    Ok(Response::new(v1::WorkspaceFiles {
      entries: entries
        .into_iter()
        .map(|entry| v1::WorkspaceEntry {
          path: entry.path,
          directory: entry.directory,
          size: entry.size,
        })
        .collect(),
    }))
  }
  async fn read_file(
    &self,
    request: Request<v1::ReadFileRequest>,
  ) -> Result<Response<v1::WorkspaceFile>, Status> {
    let input = request.into_inner();
    let path = self.workspace_path(&input.session_id)?;
    let file = input.path.clone();
    let (content, revision) =
      tokio::task::spawn_blocking(move || Workspace::open(path)?.read_revision(&input.path))
        .await
        .map_err(|_| Status::internal("workspace task failed"))?
        .map_err(failure)?;
    Ok(Response::new(v1::WorkspaceFile {
      path: file,
      content,
      revision,
    }))
  }
  async fn write_file(
    &self,
    request: Request<v1::WriteFileRequest>,
  ) -> Result<Response<v1::WorkspaceFile>, Status> {
    let input = request.into_inner();
    let path = self.workspace_path(&input.session_id)?;
    let write = self.workspace_writes.clone().lock_owned().await;
    let file = input.path.clone();
    let content = input.content.clone();
    let revision = tokio::task::spawn_blocking(move || {
      let _write = write;
      Workspace::open(path)?.write_revision(&input.path, &input.content, &input.expected_revision)
    })
    .await
    .map_err(|_| Status::internal("workspace task failed"))?
    .map_err(failure)?;
    Ok(Response::new(v1::WorkspaceFile {
      path: file,
      content,
      revision,
    }))
  }
  async fn get_diff(
    &self,
    request: Request<v1::DiffRequest>,
  ) -> Result<Response<v1::WorkspaceDiff>, Status> {
    let input = request.into_inner();
    let path = self.workspace_path(&input.session_id)?;
    let patch = diff(path, input.path).await.map_err(failure)?;
    Ok(Response::new(v1::WorkspaceDiff { patch }))
  }
  type WatchFilesStream =
    Pin<Box<dyn futures::Stream<Item = Result<v1::WorkspaceChange, Status>> + Send>>;
  async fn watch_files(
    &self,
    request: Request<v1::SessionRequest>,
  ) -> Result<Response<Self::WatchFilesStream>, Status> {
    let path = self.workspace_path(&request.into_inner().session_id)?;
    let mut watcher = WorkspaceWatch::open(std::path::Path::new(&path)).map_err(failure)?;
    let stop = self.stopping.clone();
    let stream = async_stream::stream! {
      loop {
        let change=tokio::select! {biased;_=stop.cancelled()=>break,change=watcher.next()=>change};
        let Some(change)=change else {break};
        yield Ok(v1::WorkspaceChange {kind:change.kind,paths:change.paths});
      }
    };
    Ok(Response::new(Box::pin(stream)))
  }
}

async fn diff(workspace: String, path: String) -> anyhow::Result<String> {
  use tokio::io::AsyncReadExt;
  // Validate scope through the capability filesystem before handing a path to Git.
  let root = Arc::new(Workspace::open(&workspace)?);
  let check = root.clone();
  let file = path.clone();
  if !file.is_empty() {
    tokio::task::spawn_blocking(move || check.read(&file)).await??;
  }
  let mut command = tokio::process::Command::new("git");
  command.args([
    "-c",
    "core.fsmonitor=false",
    "-c",
    "diff.external=",
    "-C",
    &workspace,
    "diff",
    "--no-ext-diff",
    "--no-textconv",
    "HEAD",
    "--",
  ]);
  if !path.is_empty() {
    command.arg(&path);
  }
  for key in [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
  ] {
    command.env_remove(key);
  }
  command
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true);
  let mut child = command.spawn()?;
  let stdout = child
    .stdout
    .take()
    .ok_or_else(|| anyhow::anyhow!("Git output unavailable"))?;
  let result = tokio::time::timeout(Duration::from_secs(5), async {
    let mut bytes = Vec::new();
    stdout
      .take(2 * 1024 * 1024 + 1)
      .read_to_end(&mut bytes)
      .await?;
    anyhow::ensure!(bytes.len() <= 2 * 1024 * 1024, "diff exceeds limit");
    anyhow::ensure!(child.wait().await?.success(), "Git diff unavailable");
    Ok::<_, anyhow::Error>(String::from_utf8(bytes)?)
  })
  .await??;
  Ok(result)
}
