use anyhow::{Context, Result, ensure};
use clap::Parser;
use config::{Config, File};
use gateway_core::{GatewayConfig, Secret};
use gateway_validation::Credentials;
use serde::Deserialize;
use std::{
  collections::BTreeMap,
  net::SocketAddr,
  path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(about = "Lyonbrown authenticated AI Gateway")]
pub struct Cli {
  #[arg(long, env = "LYONBROWN_GATEWAY_CONFIG")]
  pub config: Option<PathBuf>,
  #[arg(long, env = "LYONBROWN_GATEWAY_DATA_DIR")]
  pub data_dir: Option<PathBuf>,
  #[arg(long)]
  pub listen: Option<SocketAddr>,
}
pub struct AppConfig {
  pub listen: SocketAddr,
  pub data_dir: PathBuf,
  pub web_root: PathBuf,
  pub gateway: GatewayConfig,
  pub admin_username: String,
  pub admin_password: Secret,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSettings {
  listen: SocketAddr,
  data_dir: PathBuf,
  web_root: PathBuf,
  gateway: GatewayConfig,
}
impl AppConfig {
  pub fn load() -> Result<Self> {
    let environment = std::env::vars_os()
      .map(|(key, value)| {
        let invalid = || anyhow::anyhow!("gateway environment must be valid UTF-8");
        Ok((
          key.into_string().map_err(|_| invalid())?,
          value.into_string().map_err(|_| invalid())?,
        ))
      })
      .collect::<Result<BTreeMap<_, _>>>()?;
    let cli = Cli::parse();
    let data_dir = match &cli.data_dir {
      Some(path) => path.clone(),
      None => directories::ProjectDirs::from("com", "lyonbrown", "lyonbrown")
        .context("user data directory is unavailable")?
        .data_local_dir()
        .join("ai-gateway"),
    };
    let mut settings = Self::load_with_environment(&data_dir, cli.config.as_deref(), &environment)?;
    if cli.data_dir.is_some() {
      settings.data_dir = data_dir;
    }
    if let Some(listen) = cli.listen {
      settings.listen = listen;
    }
    settings.validate()?;
    Ok(settings)
  }
  pub fn load_with_environment(
    data_dir: &Path,
    config_path: Option<&Path>,
    environment: &BTreeMap<String, String>,
  ) -> Result<Self> {
    ensure!(
      data_dir.is_absolute(),
      "gateway data directory must be absolute"
    );
    let default_path = data_dir.join("gateway.toml");
    let path = config_path.unwrap_or(&default_path);
    let dotenv_path = path.parent().unwrap_or(data_dir).join(".env");
    let mut merged = BTreeMap::new();
    if dotenv_path.exists() {
      for item in dotenvy::from_path_iter(dotenv_path)
        .map_err(|_| anyhow::anyhow!("cannot read gateway dotenv file"))?
      {
        let (key, value) = item.map_err(|_| anyhow::anyhow!("invalid gateway dotenv file"))?;
        merged.insert(key, value);
      }
    }
    merged.extend(
      environment
        .iter()
        .map(|(key, value)| (key.clone(), value.clone())),
    );
    let mut builder = Config::builder()
      .set_default("listen", "127.0.0.1:8099")?
      .set_default(
        "data_dir",
        data_dir.to_str().context("data directory must be UTF-8")?,
      )?
      .set_default(
        "web_root",
        std::env::current_exe()
          .context("gateway executable path is unavailable")?
          .parent()
          .context("gateway executable directory is unavailable")?
          .join("web")
          .to_str()
          .context("gateway WebUI directory must be UTF-8")?,
      )?
      .add_source(File::from(path).required(config_path.is_some()));
    for (key, field) in [
      ("LYONBROWN_GATEWAY_TOKEN", "gateway.token"),
      ("LYONBROWN_GATEWAY_LISTEN", "listen"),
      ("LYONBROWN_GATEWAY_DATA_DIR", "data_dir"),
      ("LYONBROWN_GATEWAY_WEB_ROOT", "web_root"),
    ] {
      if let Some(value) = merged.get(key) {
        builder = builder.set_override(field, value.clone())?;
      }
    }
    // Configuration library errors can contain source values; keep all parser details private.
    let file: FileSettings = builder
      .build()
      .and_then(Config::try_deserialize)
      .map_err(|_| anyhow::anyhow!("invalid gateway configuration"))?;
    let mut settings = Self {
      listen: file.listen,
      data_dir: file.data_dir,
      web_root: file.web_root,
      gateway: file.gateway,
      admin_username: merged
        .get("LYONBROWN_GATEWAY_ADMIN_USERNAME")
        .context("gateway admin username environment setting is required")?
        .clone(),
      admin_password: Secret::new(
        merged
          .get("LYONBROWN_GATEWAY_ADMIN_PASSWORD")
          .context("gateway admin password environment setting is required")?
          .clone(),
      ),
    };
    for model in &mut settings.gateway.models {
      ensure!(
        model.api_key.is_none(),
        "upstream credentials must use api_key_env"
      );
      if let Some(key) = &model.api_key_env {
        let value = merged
          .get(key)
          .filter(|value| !value.is_empty())
          .context("configured upstream credential is unavailable")?;
        model.api_key = Some(Secret::new(value.clone()));
      }
    }
    settings.validate()?;
    Ok(settings)
  }
  pub fn validate(&self) -> Result<()> {
    ensure!(
      self.web_root.is_absolute(),
      "gateway WebUI directory must be absolute"
    );
    ensure!(
      self.listen.ip().is_loopback() && self.listen.port() > 0,
      "gateway listener requires loopback and a nonzero port"
    );
    ensure!(
      self.data_dir.is_absolute(),
      "gateway data directory must be absolute"
    );
    ensure!(
      Credentials::validate_username(&self.admin_username).is_ok(),
      "admin username requires 1..=64 ASCII letters, digits or _.@-"
    );
    ensure!(
      Credentials::validate_password(self.admin_password.expose_secret()).is_ok(),
      "admin password requires 15..=128 bytes"
    );
    ensure!(
      self.admin_password.expose_secret() != self.gateway.token.expose_secret(),
      "admin password and gateway token must differ"
    );
    self.gateway.validate()
  }
  pub fn validate_web_assets(&self) -> Result<()> {
    for name in ["gateway_web.js", "gateway_web.wasm"] {
      ensure!(
        self
          .web_root
          .join("ui/pkg")
          .join(name)
          .metadata()
          .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0),
        "gateway WebUI assets are missing; run gateway-dev --build-only or configure LYONBROWN_GATEWAY_WEB_ROOT"
      );
    }
    Ok(())
  }
}
