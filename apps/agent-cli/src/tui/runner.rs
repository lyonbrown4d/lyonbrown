use std::{sync::mpsc, time::Duration};

use agent_kernel::Kernel;
use agent_tool::ApprovalDecision;
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::{sync::broadcast, task::JoinHandle};

use super::{ApprovalRequest, TuiState, terminal::TerminalSession, ui::render_with_approval};

type TurnTask = JoinHandle<(
    Kernel,
    Result<Vec<agent_core::AgentEvent>, agent_core::AgentError>,
)>;

pub async fn run(kernel: Kernel, approvals: mpsc::Receiver<ApprovalRequest>) -> Result<()> {
    let mut terminal =
        TerminalSession::enter().context("failed to initialize terminal interface")?;
    let mut state = TuiState::default();
    let mut kernel = Some(kernel);
    let mut events: Option<broadcast::Receiver<agent_core::AgentEvent>> = None;
    let mut task: Option<TurnTask> = None;
    let mut approval: Option<ApprovalRequest> = None;

    loop {
        drain_events(&mut events, &mut state);
        if approval.is_none() {
            approval = approvals.try_recv().ok();
        }
        if task.as_ref().is_some_and(JoinHandle::is_finished) {
            let finished = task.take().expect("finished task exists").await;
            final_drain(&mut events, &mut state);
            match finished {
                Ok((returned_kernel, Ok(_))) => kernel = Some(returned_kernel),
                Ok((returned_kernel, Err(error))) => {
                    kernel = Some(returned_kernel);
                    state.set_error(error.to_string());
                }
                Err(error) => {
                    state.set_error(format!("turn task failed: {error}"));
                    break;
                }
            }
            events = None;
            state.busy = false;
        }

        terminal
            .terminal
            .draw(|frame| render_with_approval(frame, &state, approval.as_ref()))?;

        if event::poll(Duration::from_millis(25))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if let Some(request) = approval.take() {
                if let Some(decision) = approval_decision(key) {
                    request.respond(decision);
                } else {
                    approval = Some(request);
                }
                continue;
            }
            if quit_key(key) && !state.busy {
                break;
            }
            if state.busy {
                continue;
            }
            match key.code {
                KeyCode::Enter => {
                    let prompt = state.input.trim().to_owned();
                    if !prompt.is_empty() {
                        state.input.clear();
                        state.mark_submitted();
                        let owned_kernel =
                            kernel.take().context("kernel unavailable between turns")?;
                        events = Some(owned_kernel.subscribe());
                        task = Some(tokio::spawn(async move {
                            let mut owned_kernel = owned_kernel;
                            let result = owned_kernel.run_turn(&prompt).await;
                            (owned_kernel, result)
                        }));
                    }
                }
                KeyCode::Backspace => {
                    state.input.pop();
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    state.input.push(character);
                }
                _ => {}
            }
        }
    }
    Ok(())
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
mod tests {
    use agent_core::{AgentEvent, TurnId};
    use chrono::Utc;
    use tokio::sync::broadcast;

    use super::{TuiState, final_drain};

    #[test]
    fn final_drain_consumes_events_queued_at_task_completion() {
        let (sender, receiver) = broadcast::channel(4);
        let mut receiver = Some(receiver);
        let mut state = TuiState::default();
        sender
            .send(AgentEvent::TurnCompleted {
                turn_id: TurnId::new(),
                answer: "last answer".into(),
                ts: Utc::now(),
            })
            .unwrap();

        final_drain(&mut receiver, &mut state);

        assert_eq!(state.transcript().last().unwrap().text, "last answer");
        assert_eq!(state.status(), "idle");
    }
}
