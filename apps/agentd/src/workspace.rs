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
  async fn get_git_status(
    &self,
    request: Request<v1::SessionRequest>,
  ) -> Result<Response<v1::GitStatus>, Status> {
    let workspace = self.workspace_path(&request.into_inner().session_id)?;
    Ok(Response::new(git_status(workspace).await.map_err(failure)?))
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

async fn git_status(workspace: String) -> anyhow::Result<v1::GitStatus> {
  use tokio::io::AsyncReadExt;
  let mut command = git_command(&workspace);
  command.args([
    "status",
    "--porcelain=v2",
    "--branch",
    "-z",
    "--untracked-files=all",
  ]);
  let mut child = command.spawn()?;
  let stdout = child
    .stdout
    .take()
    .ok_or_else(|| anyhow::anyhow!("Git status output unavailable"))?;
  let output = tokio::time::timeout(Duration::from_secs(5), async {
    let mut bytes = Vec::new();
    stdout
      .take(2 * 1024 * 1024 + 1)
      .read_to_end(&mut bytes)
      .await?;
    anyhow::ensure!(bytes.len() <= 2 * 1024 * 1024, "Git status exceeds limit");
    anyhow::ensure!(child.wait().await?.success(), "Git status unavailable");
    Ok::<_, anyhow::Error>(bytes)
  })
  .await??;
  parse_git_status(&output)
}

fn parse_git_status(output: &[u8]) -> anyhow::Result<v1::GitStatus> {
  let records = output.split(|byte| *byte == 0).collect::<Vec<_>>();
  let mut status = v1::GitStatus {
    repository: true,
    ..Default::default()
  };
  let mut index = 0;
  while index < records.len() {
    let record = std::str::from_utf8(records[index])?;
    index += 1;
    if record.is_empty() {
      continue;
    }
    if let Some(value) = record.strip_prefix("# branch.head ") {
      status.branch = value.to_owned();
    } else if let Some(value) = record.strip_prefix("# branch.oid ") {
      status.head = value.to_owned();
    } else if let Some(value) = record.strip_prefix("# branch.upstream ") {
      status.upstream = value.to_owned();
    } else if let Some(value) = record.strip_prefix("# branch.ab ") {
      let mut counts = value.split_ascii_whitespace();
      status.ahead = parse_git_count(counts.next(), '+')?;
      status.behind = parse_git_count(counts.next(), '-')?;
    } else if record.starts_with("1 ") {
      status
        .entries
        .push(parse_git_entry(record, 9, String::new())?);
    } else if record.starts_with("2 ") {
      let original = records
        .get(index)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Git rename record is incomplete"))?;
      index += 1;
      status.entries.push(parse_git_entry(
        record,
        10,
        std::str::from_utf8(original)?.to_owned(),
      )?);
    } else if record.starts_with("u ") {
      status
        .entries
        .push(parse_git_entry(record, 11, String::new())?);
    } else if let Some(path) = record.strip_prefix("? ") {
      status.entries.push(v1::GitEntry {
        path: path.to_owned(),
        index_status: "?".into(),
        worktree_status: "?".into(),
        original_path: String::new(),
      });
    }
  }
  Ok(status)
}

fn parse_git_count(value: Option<&str>, prefix: char) -> anyhow::Result<i32> {
  value
    .and_then(|value| value.strip_prefix(prefix))
    .ok_or_else(|| anyhow::anyhow!("Git branch count is invalid"))?
    .parse()
    .map_err(anyhow::Error::from)
}

fn parse_git_entry(
  record: &str,
  fields: usize,
  original_path: String,
) -> anyhow::Result<v1::GitEntry> {
  let values = record.splitn(fields, ' ').collect::<Vec<_>>();
  let xy = values
    .get(1)
    .filter(|value| value.len() == 2 && value.is_ascii())
    .ok_or_else(|| anyhow::anyhow!("Git status record is invalid"))?;
  Ok(v1::GitEntry {
    path: values
      .last()
      .ok_or_else(|| anyhow::anyhow!("Git status path is missing"))?
      .to_string(),
    index_status: xy[..1].to_owned(),
    worktree_status: xy[1..].to_owned(),
    original_path,
  })
}

fn git_command(workspace: &str) -> tokio::process::Command {
  let mut command = tokio::process::Command::new("git");
  command.args(["-c", "core.fsmonitor=false", "-C", workspace]);
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
  command
}

async fn diff(workspace: String, path: String) -> anyhow::Result<String> {
  use tokio::io::AsyncReadExt;
  // Validate scope through the capability filesystem before handing a path to Git.
  let root = Arc::new(Workspace::open(&workspace)?);
  let check = root.clone();
  let file = path.clone();
  if !file.is_empty() {
    tokio::task::spawn_blocking(move || check.validate_diff_path(&file)).await??;
  }
  let mut command = git_command(&workspace);
  command.args([
    "--literal-pathspecs",
    "-c",
    "diff.external=",
    "diff",
    "--no-ext-diff",
    "--no-textconv",
    "HEAD",
    "--",
  ]);
  if !path.is_empty() {
    command.arg(&path);
  }
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

#[cfg(test)]
mod tests {
  use super::parse_git_status;

  #[test]
  fn parses_porcelain_v2_branch_changes_and_rename() {
    let status = parse_git_status(
      b"# branch.oid abc123\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\x001 M. N... 100644 100644 100644 a b src/lib.rs\x002 R. N... 100644 100644 100644 a b R100 src/new.rs\0src/old.rs\0? notes.txt\0",
    )
    .expect("valid fixture");
    assert!(status.repository);
    assert_eq!(status.branch, "main");
    assert_eq!(status.ahead, 2);
    assert_eq!(status.behind, 1);
    assert_eq!(status.entries.len(), 3);
    assert_eq!(status.entries[0].path, "src/lib.rs");
    assert_eq!(status.entries[1].original_path, "src/old.rs");
    assert_eq!(status.entries[2].index_status, "?");
  }

  #[test]
  fn rejects_incomplete_rename_record() {
    let result = parse_git_status(
      b"# branch.head main\x002 R. N... 100644 100644 100644 a b R100 src/new.rs\0",
    );
    assert!(result.is_err());
  }
}
