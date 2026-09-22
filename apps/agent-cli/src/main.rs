use agent_cli::cli::Cli;
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    agent_cli::cli::run(Cli::parse()).await
}
