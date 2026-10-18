use agent_dev::{Options, Plan, Target};
use anyhow::{Context, Result, ensure};
use std::{
  collections::BTreeMap,
  net::TcpListener,
  path::{Path, PathBuf},
};

pub fn plan(root: &Path) -> Result<Plan> {
  let listen = TcpListener::bind("127.0.0.1:0")?.local_addr()?;
  let mut plan = Plan::new(
    root,
    &Options {
      target: Target::Tui,
      data_dir: Some("data with spaces".into()),
      config: None,
      listen,
      dry_run: false,
    },
  )?;
  plan.prepare(&BTreeMap::new())?;
  Ok(plan)
}
pub fn binary() -> Result<PathBuf> {
  let path = std::env::var_os("LYONBROWN_DEV_TEST_AGENTD")
    .map(PathBuf::from)
    .context("set LYONBROWN_DEV_TEST_AGENTD to the separately built agentd executable")?;
  ensure!(
    path.is_absolute() && path.is_file(),
    "test daemon executable must be an absolute file path"
  );
  Ok(path)
}
