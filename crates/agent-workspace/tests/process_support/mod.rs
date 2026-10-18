use agent_workspace::process::*;
use async_trait::async_trait;
use std::{
  collections::BTreeMap,
  path::{Path, PathBuf},
  sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

pub fn fixture() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_agent-process-fixture"))
    .canonicalize()
    .unwrap()
}
pub fn request(arguments: Vec<String>) -> CommandRequest {
  CommandRequest {
    action_id: "action-fixture".into(),
    executable: fixture(),
    arguments,
    cwd: ".".into(),
    timeout_ms: 5000,
    output_limit_bytes: 16384,
  }
}
pub fn policy() -> CommandPolicy {
  CommandPolicy::new(
    [fixture()],
    BTreeMap::from([("LYONBROWN_FIXTURE_SELECTED".into(), "chosen".into())]),
  )
  .unwrap()
}
#[derive(Default)]
pub struct Approval {
  pub allow: bool,
  pub seen: Mutex<Vec<(CommandRequest, ResolvedCommand)>>,
}
#[async_trait]
impl CommandAuthorization for Approval {
  async fn authorize(
    &self,
    request: &CommandRequest,
    resolved: &ResolvedCommand,
  ) -> anyhow::Result<bool> {
    self
      .seen
      .lock()
      .unwrap()
      .push((request.clone(), resolved.clone()));
    Ok(self.allow)
  }
}
pub fn executor(root: &Path) -> CommandExecutor {
  CommandExecutor::open(
    root,
    policy(),
    Arc::new(Approval {
      allow: true,
      ..Approval::default()
    }),
  )
  .unwrap()
}
pub async fn run(root: &Path, arguments: Vec<String>) -> CommandOutcome {
  executor(root)
    .execute(request(arguments), CancellationToken::new())
    .await
}
pub async fn wait_ready(path: &Path) {
  tokio::time::timeout(std::time::Duration::from_secs(3), async {
    while !path.exists() {
      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
}
pub async fn no_descendant_effect(marker: &Path) {
  tokio::time::sleep(std::time::Duration::from_millis(1400)).await;
  assert!(!marker.exists(), "descendant survived process tree cleanup");
}

#[derive(Default)]
pub struct ApprovalGate {
  pub entered: CancellationToken,
  pub release: CancellationToken,
}
#[async_trait]
impl CommandAuthorization for ApprovalGate {
  async fn authorize(&self, _: &CommandRequest, _: &ResolvedCommand) -> anyhow::Result<bool> {
    self.entered.cancel();
    self.release.cancelled().await;
    Ok(true)
  }
}
