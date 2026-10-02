use agentd::args::Args;
use anyhow::Result;
use clap::Parser as _;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
  tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .with_target(false)
    .try_init()
    .ok();
  agentd::runtime::run(Args::parse()).await
}
