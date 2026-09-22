use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use agent_model::{
    ChatMessage, ModelEvent, ModelRegistry, ModelRequest, ModelScheduler, Role, RoutingPolicy,
    StepContext,
};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use provider_ollama::OllamaProvider;

use crate::config::{CliOverrides, LoadOptions, RuntimeConfig};

#[derive(Debug, Parser)]
#[command(name = "agent", version, about = "Local-first agent runtime")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Explicit configuration file (TOML, YAML, JSON, JSON5, RON, or INI).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[arg(long, global = true)]
    pub provider: Option<String>,

    #[arg(long, global = true)]
    pub model: Option<String>,

    /// Ollama base URL.
    #[arg(long, global = true)]
    pub endpoint: Option<String>,

    /// Allow all tool requests for this session (tools are introduced in M3).
    #[arg(long, global = true)]
    pub approve_all: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Ask one question and stream the answer.
    Ask { prompt: String },
    /// Start a multi-turn terminal chat.
    Chat,
}

pub async fn run(cli: Cli) -> Result<()> {
    let workspace = std::env::current_dir().context("failed to determine workspace directory")?;
    let mut options = LoadOptions::for_workspace(workspace);
    options.explicit_config = cli.config;
    options.cli = CliOverrides {
        provider: cli.provider,
        model: cli.model,
        endpoint: cli.endpoint,
        approve_all: cli.approve_all.then_some(true),
        ..Default::default()
    };
    let config = RuntimeConfig::load(options)?;

    match cli.command {
        Command::Ask { prompt } => ask(&config, &prompt).await,
        Command::Chat => chat(&config).await,
    }
}

async fn ask(config: &RuntimeConfig, prompt: &str) -> Result<()> {
    let mut messages = Vec::new();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    stream_turn(config, prompt, &mut messages, &mut output).await?;
    writeln!(output)?;
    Ok(())
}

async fn chat(config: &RuntimeConfig) -> Result<()> {
    let mut messages = Vec::new();
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut prompt = String::new();
        if io::stdin().read_line(&mut prompt)? == 0 {
            break;
        }
        let prompt = prompt.trim();
        if prompt.is_empty() {
            continue;
        }
        if matches!(prompt, "/exit" | "/quit") {
            break;
        }
        let stdout = io::stdout();
        let mut output = stdout.lock();
        stream_turn(config, prompt, &mut messages, &mut output).await?;
        writeln!(output)?;
    }
    Ok(())
}

async fn stream_turn(
    config: &RuntimeConfig,
    prompt: &str,
    messages: &mut Vec<ChatMessage>,
    output: &mut impl Write,
) -> Result<()> {
    let registry = ModelRegistry::new();
    registry.register(Arc::new(OllamaProvider::with_default_models(
        &config.ollama.endpoint,
    )?));
    let scheduler = ModelScheduler::new(registry, RoutingPolicy::default());
    let mut context = StepContext::new(prompt);
    context.provider = Some(config.model.provider.clone());
    context.model = config.model.model.clone();
    let selection = scheduler.decide(&context).await?;

    messages.push(ChatMessage {
        role: Role::User,
        content: prompt.to_owned(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: vec![],
    });
    let request = ModelRequest {
        model: selection.capabilities.model.clone(),
        system: None,
        messages: messages.clone(),
        tools: vec![],
        max_tokens: config
            .model
            .max_tokens
            .min(selection.capabilities.max_output_tokens),
        temperature: config.model.temperature,
        json_mode: false,
        thinking: selection.capabilities.supports_thinking.then_some(false),
        stop: vec![],
    };
    let mut events = selection.provider.stream(request).await?.events;
    let mut answer = String::new();
    let mut done = false;
    while let Some(event) = events.next().await {
        match event? {
            ModelEvent::Delta(delta) => {
                output.write_all(delta.as_bytes())?;
                output.flush()?;
                answer.push_str(&delta);
            }
            ModelEvent::ToolCall(call) => {
                bail!(
                    "model requested tool '{}', but CLI tool execution is not enabled until M3",
                    call.name
                );
            }
            ModelEvent::Usage(_) => {}
            ModelEvent::Done { .. } => done = true,
        }
    }
    if !done {
        bail!("provider stream ended without a Done event");
    }
    messages.push(ChatMessage {
        role: Role::Assistant,
        content: answer,
        tool_call_id: None,
        tool_name: None,
        tool_calls: vec![],
    });
    Ok(())
}
