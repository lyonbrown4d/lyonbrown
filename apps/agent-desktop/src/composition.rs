use agent_client::AgentClient;
use fluxdi::{Error, Shared, static_module};
use std::{path::PathBuf, sync::Arc};

pub struct DesktopConfiguration {
  pub settings: agent_config::Settings,
  pub data_dir: PathBuf,
  pub browser_executable: Option<PathBuf>,
}

pub struct ExecutorPort(pub Arc<tokio::runtime::Runtime>);
pub struct ClientPort(Shared<DesktopConfiguration>);
pub struct BrowserPort {
  pub executable: PathBuf,
  pub profiles: PathBuf,
}
impl BrowserPort {
  pub async fn attach(
    &self,
    client: AgentClient,
  ) -> anyhow::Result<agent_browser_runtime::RuntimeAttachment> {
    agent_browser_runtime::attach_executable(client, &self.executable, &self.profiles).await
  }
}
fn browser(configuration: Shared<DesktopConfiguration>) -> Result<Shared<BrowserPort>, Error> {
  let executable = configuration
    .browser_executable
    .clone()
    .map_or_else(std::env::current_exe, Ok)
    .map_err(|error| Error::factory_failed("BrowserExecutable", &error.to_string()))?;
  Ok(Shared::new(BrowserPort {
    executable,
    profiles: configuration
      .data_dir
      .join("browser-profiles")
      .join("desktop"),
  }))
}

impl ClientPort {
  pub async fn connect(&self, endpoint: &str, token: &str) -> anyhow::Result<AgentClient> {
    if endpoint == agent_launch::model::LOCAL_ENDPOINT {
      let mut options = agent_launch::LaunchOptions::bundled(["sessions", "runs"])?;
      options.data_dir = self.0.data_dir.clone();
      agent_launch::ensure_daemon(options).await
    } else {
      AgentClient::connect(endpoint, token).await
    }
  }
}

fn executor() -> Result<Shared<ExecutorPort>, Error> {
  tokio::runtime::Builder::new_multi_thread()
    .worker_threads(2)
    .enable_all()
    .build()
    .map(|runtime| Shared::new(ExecutorPort(Arc::new(runtime))))
    .map_err(|error| Error::factory_failed("DesktopExecutor", &error.to_string()))
}

fn client(configuration: Shared<DesktopConfiguration>) -> Result<Shared<ClientPort>, Error> {
  Ok(Shared::new(ClientPort(configuration)))
}

static_module! {
  pub struct DesktopModule {
    external configuration: DesktopConfiguration;
    #[scope(root)]
    executor: ExecutorPort = executor();
    #[scope(root)]
    client: ClientPort = client(configuration);
    #[scope(root)]
    browser: BrowserPort = browser(configuration);
  }
}
