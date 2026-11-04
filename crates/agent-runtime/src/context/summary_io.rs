use super::operations::check_intent;
use crate::{Result, Runtime, RuntimeError, SummaryInput};
use futures::StreamExt;
use rig_core::{
  message::{AssistantContent, Message, UserContent},
  streaming::{Item, PartKind, StreamEvent},
};
use rig_memory::{Summarizer, SummaryPlan};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const SUMMARY_BYTES: usize = 128 * 1024;

impl Runtime {
  pub(super) async fn summary_request(
    &self,
    id: &str,
    revision: u64,
    plan: &SummaryPlan,
    token: &CancellationToken,
  ) -> Result<String> {
    let mut system_prompt = String::new();
    let mut transcript = String::new();
    for message in &plan.request.chat_history {
      match &**message {
        Message::System { content } => system_prompt.push_str(content),
        Message::User { content } => {
          for item in content {
            let UserContent::Text(text) = item else {
              return Err(RuntimeError::Execution(
                "summary request contains non-text input".into(),
              ));
            };
            transcript.push_str(&text.text);
          }
        }
        _ => {
          return Err(RuntimeError::Execution(
            "unsupported summary request shape".into(),
          ));
        }
      }
    }
    let preparation = async {
      let input = SummaryInput {
        system_prompt,
        transcript,
      };
      let input = match &self.summary_guard {
        Some(guard) => guard.prepare(input, token.clone()).await?,
        None => input,
      };
      let (system_prompt, transcript) = self
        .protect_summary_input(&input.system_prompt, &input.transcript)
        .await?;
      Ok::<_, RuntimeError>(SummaryInput {
        system_prompt,
        transcript,
      })
    };
    let guarded = tokio::select! { biased;
      _ = token.cancelled() => return Err(RuntimeError::Execution("summary cancelled before provider IO".into())),
      _ = self.session.stopped.cancelled() => return Err(RuntimeError::Uncertain("session writer fenced".into())),
      result = tokio::time::timeout(Duration::from_secs(30), preparation) => result.map_err(|_| RuntimeError::Execution("summary preparation timed out".into()))??,
    };
    if token.is_cancelled() {
      return Err(RuntimeError::Execution(
        "summary cancelled before provider IO".into(),
      ));
    }
    let mut request = plan.request.clone();
    request.chat_history = vec![
      Arc::new(Message::system(guarded.system_prompt)),
      Arc::new(Message::user(guarded.transcript)),
    ];
    // Neither a summary request nor its answer can register or invoke any tool.
    request.tools.clear();
    request.tool_choice = None;
    request.record_telemetry_content = false;
    if rig_memory::estimate_tokens(&request.chat_history)
      .saturating_add(request.max_tokens.unwrap_or_default())
      >= u64::from(
        self
          .context_spec()?
          .input_limit()
          .unwrap_or(self.config.context_window),
      )
    {
      return Err(RuntimeError::Configuration(
        "prepared summary exceeds model budget".into(),
      ));
    }
    self
      .session
      .update("context_summary_sending", |state| {
        check_intent(state, id, revision)?;
        if let Some(intent) = state.context.pending.as_mut() {
          intent.sending = true;
        }
        Ok(((), json!({"attempt_id":id,"revision":revision}), vec![]))
      })
      .await?;
    let mut stream = self.model.stream(request).map_err(|_| unknown())?;
    let deadline = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(deadline);
    let mut bytes = 0_usize;
    let mut received = 0_usize;
    loop {
      let item = tokio::select! { biased;
        _ = self.session.stopped.cancelled() => return Err(unknown()),
        _ = token.cancelled() => return Err(unknown()),
        _ = &mut deadline => return Err(unknown()),
        item = stream.next() => item,
      };
      let Some(item) = item else {
        break;
      };
      let item = item.map_err(|_| unknown())?;
      received += crate::budget::size(&item, crate::budget::MODEL_BYTES.saturating_sub(received))
        .map_err(|_| unknown())?;
      match item {
        Item::Event(StreamEvent::Text { text, .. } | StreamEvent::Reasoning { text, .. }) => {
          bytes = bytes.saturating_add(text.len());
          if bytes > SUMMARY_BYTES {
            return Err(unknown());
          }
        }
        Item::Event(
          StreamEvent::Start {
            kind: PartKind::ToolCall,
            ..
          }
          | StreamEvent::Arguments { .. }
          | StreamEvent::End {
            content: AssistantContent::ToolCall(_),
            ..
          },
        ) => {
          return Err(unknown());
        }
        _ => {}
      }
    }
    let response = tokio::select! { biased;
      _ = self.session.stopped.cancelled() => return Err(unknown()),
      _ = token.cancelled() => return Err(unknown()),
      _ = &mut deadline => return Err(unknown()),
      result = stream.finish() => result.map_err(|_| unknown())?,
    };
    crate::budget::check(&response.choice, crate::budget::MODEL_BYTES).map_err(|_| unknown())?;
    if response.tool_calls().next().is_some() {
      return Err(RuntimeError::Execution(
        "summary returned a forbidden tool call".into(),
      ));
    }
    let summary = Summarizer::summary_text(&response).map_err(|_| {
      RuntimeError::Execution("model did not return a complete, tool-free summary".into())
    })?;
    if summary.len() > SUMMARY_BYTES {
      return Err(RuntimeError::Execution(
        "summary output exceeds 128KiB".into(),
      ));
    }
    self.validate_summary_output(&summary).await?;
    Ok(summary)
  }
}

fn unknown() -> RuntimeError {
  RuntimeError::Uncertain("summary provider outcome was not confirmed; never replay".into())
}
