use std::collections::HashMap;
use std::path::PathBuf;

use agent_core::SecretRef;
use config::{Config, Environment, File};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub profile: String,
    pub model: ModelConfig,
    pub ollama: OllamaConfig,
    pub llamacpp: LlamaCppRuntimeConfig,
    pub approval: ApprovalConfig,
    #[serde(default)]
    pub secrets: Vec<SecretRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub provider: String,
    pub model: Option<String>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OllamaConfig {
    pub endpoint: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlamaCppRuntimeConfig {
    pub model_path: Option<PathBuf>,
    pub context_size: u32,
    pub max_output_tokens: u32,
    pub batch_size: u32,
    pub threads: u32,
    pub batch_threads: u32,
    pub gpu_layers: u32,
    pub temperature: f32,
    pub seed: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalConfig {
    pub approve_all: bool,
    pub deny_all: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    pub profile: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub approve_all: Option<bool>,
    pub deny_all: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct LoadOptions {
    pub global_config: PathBuf,
    pub workspace_dir: PathBuf,
    pub explicit_config: Option<PathBuf>,
    /// `None` reads the process environment. Tests can inject an isolated map.
    pub environment: Option<HashMap<String, String>>,
    pub cli: CliOverrides,
}

impl LoadOptions {
    pub fn for_workspace(workspace_dir: impl Into<PathBuf>) -> Self {
        let global_config = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".agent")
            .join("config.toml");
        Self {
            global_config,
            workspace_dir: workspace_dir.into(),
            explicit_config: None,
            environment: None,
            cli: CliOverrides::default(),
        }
    }
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self::for_workspace(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

impl RuntimeConfig {
    pub fn load(options: LoadOptions) -> Result<Self, ConfigLoadError> {
        let workspace_config = options.workspace_dir.join("agent.toml");
        let environment = Environment::with_prefix("LYONBROWN")
            .prefix_separator("_")
            .separator("__")
            .try_parsing(true)
            .source(options.environment);

        let mut builder = Config::builder()
            .set_default("profile", "default-dev")?
            .set_default("model.provider", "ollama")?
            .set_default("model.max_tokens", 2_048_u32)?
            .set_default("ollama.endpoint", "http://127.0.0.1:11434")?
            .set_default("llamacpp.model_path", Option::<String>::None)?
            .set_default("llamacpp.context_size", 4_096_u32)?
            .set_default("llamacpp.max_output_tokens", 2_048_u32)?
            .set_default("llamacpp.batch_size", 512_u32)?
            .set_default("llamacpp.threads", 4_u32)?
            .set_default("llamacpp.batch_threads", 4_u32)?
            .set_default("llamacpp.gpu_layers", 0_u32)?
            .set_default("llamacpp.temperature", 0.7_f64)?
            .set_default("llamacpp.seed", 0_u32)?
            .set_default("approval.approve_all", false)?
            .set_default("approval.deny_all", false)?
            .set_default("secrets", Vec::<String>::new())?
            .add_source(File::from(options.global_config).required(false))
            .add_source(File::from(workspace_config).required(false));
        if let Some(explicit_config) = options.explicit_config {
            builder = builder.add_source(File::from(explicit_config));
        }
        builder = builder.add_source(environment);
        builder = apply_cli_overrides(builder, options.cli)?;

        let loaded: Self = builder.build()?.try_deserialize()?;
        loaded.validate()?;
        Ok(loaded)
    }

    fn validate(&self) -> Result<(), ConfigLoadError> {
        if self.profile.trim().is_empty() {
            return Err(ConfigLoadError::Validation(
                "profile must not be empty".into(),
            ));
        }
        if self.model.provider.trim().is_empty() {
            return Err(ConfigLoadError::Validation(
                "model.provider must not be empty".into(),
            ));
        }
        if self.model.max_tokens == 0 {
            return Err(ConfigLoadError::Validation(
                "model.max_tokens must be greater than zero".into(),
            ));
        }
        if self.model.provider == "llamacpp" && self.llamacpp.model_path.is_none() {
            return Err(ConfigLoadError::Validation(
                "llamacpp.model_path is required when model.provider is 'llamacpp'".into(),
            ));
        }
        if self.llamacpp.context_size == 0 {
            return Err(ConfigLoadError::Validation(
                "llamacpp.context_size must be greater than zero".into(),
            ));
        }
        if self.llamacpp.max_output_tokens == 0
            || self.llamacpp.max_output_tokens > self.llamacpp.context_size
        {
            return Err(ConfigLoadError::Validation(
                "llamacpp.max_output_tokens must be between 1 and llamacpp.context_size".into(),
            ));
        }
        if self.llamacpp.threads == 0 || self.llamacpp.batch_threads == 0 {
            return Err(ConfigLoadError::Validation(
                "llamacpp thread counts must be greater than zero".into(),
            ));
        }
        if self.llamacpp.batch_size == 0 || self.llamacpp.batch_size > self.llamacpp.context_size {
            return Err(ConfigLoadError::Validation(
                "llamacpp.batch_size must be between 1 and llamacpp.context_size".into(),
            ));
        }
        if !self.llamacpp.temperature.is_finite() || self.llamacpp.temperature < 0.0 {
            return Err(ConfigLoadError::Validation(
                "llamacpp.temperature must be finite and non-negative".into(),
            ));
        }
        if let Some(temperature) = self.model.temperature
            && (!temperature.is_finite() || !(0.0..=2.0).contains(&temperature))
        {
            return Err(ConfigLoadError::Validation(
                "model.temperature must be between 0 and 2".into(),
            ));
        }
        if self.approval.approve_all && self.approval.deny_all {
            return Err(ConfigLoadError::Validation(
                "approval.approve_all and approval.deny_all cannot both be true".into(),
            ));
        }
        for reference in &self.secrets {
            let (kind, name) = reference.parts();
            if !matches!(kind, "env" | "file") || name.is_empty() {
                return Err(ConfigLoadError::Validation(format!(
                    "invalid secret reference '{reference}'; expected env:NAME or file:NAME"
                )));
            }
        }
        Ok(())
    }
}

fn apply_cli_overrides(
    mut builder: config::ConfigBuilder<config::builder::DefaultState>,
    overrides: CliOverrides,
) -> Result<config::ConfigBuilder<config::builder::DefaultState>, config::ConfigError> {
    if let Some(value) = overrides.profile {
        builder = builder.set_override("profile", value)?;
    }
    if let Some(value) = overrides.provider {
        builder = builder.set_override("model.provider", value)?;
    }
    if let Some(value) = overrides.model {
        builder = builder.set_override("model.model", value)?;
    }
    if let Some(value) = overrides.endpoint {
        builder = builder.set_override("ollama.endpoint", value)?;
    }
    if let Some(value) = overrides.max_tokens {
        builder = builder.set_override("model.max_tokens", value)?;
    }
    if let Some(value) = overrides.temperature {
        builder = builder.set_override("model.temperature", f64::from(value))?;
    }
    if let Some(value) = overrides.approve_all {
        builder = builder.set_override("approval.approve_all", value)?;
    }
    if let Some(value) = overrides.deny_all {
        builder = builder.set_override("approval.deny_all", value)?;
    }
    Ok(builder)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigLoadError {
    #[error("configuration error: {0}")]
    Config(#[from] config::ConfigError),
    #[error("configuration error: {0}")]
    Validation(String),
}
