use agent_dev::gateway::{GatewayOptions, GatewayPlan, repository_root};
use anyhow::Result;
use clap::Parser;
use std::collections::BTreeMap;

#[tokio::main]
async fn main() -> Result<()> {
  let options = GatewayOptions::parse();
  let root = repository_root()?;
  let mut plan = GatewayPlan::new(&root, &options)?;
  plan.print();
  if options.dry_run {
    return Ok(());
  }
  let _logs = agent_telemetry::init(&root.join(".local/gateway-web"), "gateway-dev", "info")?;
  let environment: BTreeMap<_, _> = std::env::vars_os().collect();
  let binary = plan.build(&environment).await?;
  if !options.build_only {
    plan.serve(&binary).await?;
  }
  Ok(())
}
