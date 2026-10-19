use anyhow::Result;
use model_mock::{AppConfig, Cli, MockApp};

#[tokio::main]
async fn main() -> Result<()> {
  let settings = AppConfig::load(Cli::parse_args())?;
  let _logs = agent_telemetry::init(&settings.data_dir, "model-mock", "info,wiremock=off")?;
  let app = MockApp::start(settings).await?;
  tracing::info!(address=%app.address(), event="model_mock_ready", "Synthetic model server ready");
  println!("model-mock ready at {}", app.base_url());
  tokio::signal::ctrl_c().await?;
  app.shutdown().await;
  Ok(())
}
