use std::{sync::mpsc, time::Duration};

use agent_kernel::Kernel;
use agent_tool::ApprovalDecision;
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use super::{ApprovalRequest, TuiState, terminal::TerminalSession, ui::render_with_approval};

type TurnTask = JoinHandle<(
    Kernel,
    Result<Vec<agent_core::AgentEvent>, agent_core::AgentError>,
)>;

struct Runtime {
    state: TuiState,
    kernel: Option<Kernel>,
    events: Option<broadcast::Receiver<agent_core::AgentEvent>>,
    task: Option<TurnTask>,
    approval: Option<ApprovalRequest>,
    approvals: Option<mpsc::Receiver<ApprovalRequest>>,
    approval_scroll: u16,
    approval_max_scroll: u16,
}

pub async fn run(kernel: Kernel, approvals: mpsc::Receiver<ApprovalRequest>) -> Result<()> {
    let cancellation = kernel.cancellation_token();
    let mut terminal = match TerminalSession::enter() {
        Ok(terminal) => terminal,
        Err(error) => {
            cancellation.cancel();
            return Err(error).context("failed to initialize terminal interface");
        }
    };
    let mut runtime = Runtime {
        state: TuiState::default(),
        kernel: Some(kernel),
        events: None,
        task: None,
        approval: None,
        approvals: Some(approvals),
        approval_scroll: 0,
        approval_max_scroll: 0,
    };

    let result = run_event_loop(&mut terminal, &mut runtime).await;
    shutdown(&mut runtime, &cancellation).await;
    result
}

async fn run_event_loop(terminal: &mut TerminalSession, runtime: &mut Runtime) -> Result<()> {
    loop {
        drain_events(&mut runtime.events, &mut runtime.state);
        if runtime.approval.is_none()
            && let Some(approvals) = runtime.approvals.as_ref()
            && let Ok(request) = approvals.try_recv()
        {
            runtime.approval = Some(request);
            runtime.approval_scroll = 0;
        }
        if runtime.task.as_ref().is_some_and(JoinHandle::is_finished) {
            let finished = runtime.task.take().expect("finished task exists").await;
            final_drain(&mut runtime.events, &mut runtime.state);
            match finished {
                Ok((returned_kernel, Ok(_))) => runtime.kernel = Some(returned_kernel),
                Ok((returned_kernel, Err(error))) => {
                    runtime.kernel = Some(returned_kernel);
                    runtime.state.set_error(error.to_string());
                }
                Err(error) => {
                    runtime
                        .state
                        .set_error(format!("turn task failed: {error}"));
                    anyhow::bail!("turn task failed: {error}");
                }
            }
            runtime.events = None;
        }
        runtime.state.busy = runtime.task.is_some();

        let mut approval_max_scroll = 0;
        terminal.terminal.draw(|frame| {
            approval_max_scroll = render_with_approval(
                frame,
                &runtime.state,
                runtime.approval.as_ref(),
                runtime.approval_scroll,
            );
        })?;
        runtime.approval_max_scroll = approval_max_scroll;
        runtime.approval_scroll = runtime.approval_scroll.min(approval_max_scroll);

        if event::poll(Duration::from_millis(25))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if quit_key(key) {
                return Ok(());
            }
            if runtime.approval.is_some() {
                match key.code {
                    KeyCode::Up => {
                        runtime.approval_scroll = runtime.approval_scroll.saturating_sub(1)
                    }
                    KeyCode::Down => {
                        runtime.approval_scroll = runtime
                            .approval_scroll
                            .saturating_add(1)
                            .min(runtime.approval_max_scroll)
                    }
                    KeyCode::PageUp => {
                        runtime.approval_scroll = runtime.approval_scroll.saturating_sub(5)
                    }
                    KeyCode::PageDown => {
                        runtime.approval_scroll = runtime
                            .approval_scroll
                            .saturating_add(5)
                            .min(runtime.approval_max_scroll)
                    }
                    KeyCode::Home => runtime.approval_scroll = 0,
                    KeyCode::End => runtime.approval_scroll = runtime.approval_max_scroll,
                    _ => {}
                }
                if let Some(decision) = approval_decision(key) {
                    runtime
                        .approval
                        .take()
                        .expect("approval exists")
                        .respond(decision);
                }
                continue;
            }
            if !can_accept_input(runtime.task.is_some(), runtime.kernel.is_some()) {
                continue;
            }
            match key.code {
                KeyCode::Enter => {
                    let prompt = runtime.state.input.trim().to_owned();
                    if !prompt.is_empty() {
                        runtime.state.input.clear();
                        runtime.state.mark_submitted();
                        let owned_kernel = runtime
                            .kernel
                            .take()
                            .context("kernel unavailable between turns")?;
                        runtime.events = Some(owned_kernel.subscribe());
                        runtime.task = Some(tokio::spawn(async move {
                            let mut owned_kernel = owned_kernel;
                            let result = owned_kernel.run_turn(&prompt).await;
                            (owned_kernel, result)
                        }));
                    }
                }
                KeyCode::Backspace => {
                    runtime.state.input.pop();
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    runtime.state.input.push(character);
                }
                _ => {}
            }
        }
    }
}

async fn shutdown(runtime: &mut Runtime, cancellation: &CancellationToken) {
    if let Some(request) = runtime.approval.take() {
        request.respond(ApprovalDecision::Deny);
    }
    runtime.approvals.take();
    cancellation.cancel();

    if let Some(mut task) = runtime.task.take() {
        if tokio::time::timeout(Duration::from_secs(2), &mut task)
            .await
            .is_err()
        {
            task.abort();
            let _ = task.await;
        }
        final_drain(&mut runtime.events, &mut runtime.state);
    }
}

fn can_accept_input(task_active: bool, kernel_available: bool) -> bool {
    !task_active && kernel_available
}

fn final_drain(
    receiver: &mut Option<broadcast::Receiver<agent_core::AgentEvent>>,
    state: &mut TuiState,
) {
    drain_events(receiver, state);
}

fn drain_events(
    receiver: &mut Option<broadcast::Receiver<agent_core::AgentEvent>>,
    state: &mut TuiState,
) {
    let Some(receiver) = receiver else { return };
    loop {
        match receiver.try_recv() {
            Ok(event) => state.reduce(event),
            Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed) => {
                break;
            }
            Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                state.set_error(format!("event stream lagged by {skipped} events"));
            }
        }
    }
}

fn quit_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn approval_decision(key: KeyEvent) -> Option<ApprovalDecision> {
    match key.code {
        KeyCode::Char('y') => Some(ApprovalDecision::Allow),
        KeyCode::Char('n') => Some(ApprovalDecision::Deny),
        KeyCode::Char('a') => Some(ApprovalDecision::AllowSession),
        KeyCode::Char('d') => Some(ApprovalDecision::DenyAll),
        _ => None,
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
