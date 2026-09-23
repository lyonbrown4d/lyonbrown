use std::io::{self, Write};

use agent_core::{AgentError, AgentEvent};
use agent_kernel::Kernel;
use anyhow::Result;

pub(super) async fn ask(kernel: &mut Kernel, prompt: &str) -> Result<()> {
  let stdout = io::stdout();
  let mut output = stdout.lock();
  run_interruptible_turn(kernel, prompt, &mut output).await?;
  writeln!(output)?;
  Ok(())
}

pub(super) async fn chat(kernel: &mut Kernel) -> Result<()> {
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
    if run_interruptible_turn(kernel, prompt, &mut output).await? {
      break;
    }
    writeln!(output)?;
  }
  Ok(())
}

async fn run_interruptible_turn(
  kernel: &mut Kernel,
  prompt: &str,
  output: &mut impl Write,
) -> Result<bool> {
  let cancellation = kernel.cancellation_token();
  let turn = run_kernel_turn(kernel, prompt, output);
  tokio::pin!(turn);
  tokio::select! {
    result = &mut turn => {
      result?;
      Ok(false)
    }
    signal = tokio::signal::ctrl_c() => {
      signal?;
      cancellation.cancel();
      tokio::select! {
        result = &mut turn => match result {
          Ok(_) | Err(AgentError::Cancelled) => Ok(true),
          Err(error) => Err(error.into()),
        },
        signal = tokio::signal::ctrl_c() => {
          signal?;
          anyhow::bail!("forced interrupt")
        }
      }
    }
  }
}

pub async fn run_kernel_turn(
  kernel: &mut Kernel,
  prompt: &str,
  output: &mut impl Write,
) -> Result<Vec<AgentEvent>, AgentError> {
  let mut events = kernel.subscribe();
  let turn = kernel.run_turn(prompt);
  tokio::pin!(turn);
  loop {
    tokio::select! {
      biased;
      event = events.recv() => {
        match event {
          Ok(event) => render_event(&event, output)?,
          Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
            return Err(AgentError::Tool(format!(
              "CLI event renderer lagged by {skipped} events"
            )));
          }
          Err(tokio::sync::broadcast::error::RecvError::Closed) => {
            return Err(AgentError::Tool("kernel event stream closed".into()));
          }
        }
      }
      result = &mut turn => {
        let result = result?;
        loop {
          match events.try_recv() {
            Ok(event) => render_event(&event, output)?,
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
              return Err(AgentError::Tool(format!(
                "CLI event renderer lagged by {skipped} events"
              )));
            }
          }
        }
        return Ok(result);
      },
    }
  }
}

fn render_event(event: &AgentEvent, output: &mut impl Write) -> Result<(), AgentError> {
  if let AgentEvent::ModelDelta { delta, .. } = event {
    let delta = crate::tui::terminal_safe(delta);
    output
      .write_all(delta.as_bytes())
      .map_err(|error| AgentError::Tool(error.to_string()))?;
    output
      .flush()
      .map_err(|error| AgentError::Tool(error.to_string()))?;
  }
  Ok(())
}
