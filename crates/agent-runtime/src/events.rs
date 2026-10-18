use futures::future::BoxFuture;
use rpi_agent::events::{AgentEmitter, AgentEvent};
use rpi_agent::types::TextContentOrImage;
use rpi_ai::AssistantMessageEvent;

use crate::{EventListener, RuntimeEvent};

pub(crate) struct Emitter(pub EventListener);

impl AgentEmitter for Emitter {
  fn emit(&self, event: AgentEvent) -> BoxFuture<'static, ()> {
    self.try_emit(event);
    Box::pin(async {})
  }

  fn try_emit(&self, event: AgentEvent) {
    let mapped = match event {
      AgentEvent::MessageUpdate {
        assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
        ..
      } => RuntimeEvent::TextDelta { text: delta },
      AgentEvent::MessageUpdate {
        assistant_message_event: AssistantMessageEvent::ThinkingDelta { delta, .. },
        ..
      } => RuntimeEvent::ThinkingDelta { text: delta },
      AgentEvent::ToolExecutionStart {
        tool_call_id,
        tool_name,
        args,
      } => RuntimeEvent::ToolStarted {
        call_id: tool_call_id,
        name: tool_name,
        arguments: args,
      },
      AgentEvent::ToolExecutionEnd {
        tool_call_id,
        tool_name,
        result,
        is_error,
      } => RuntimeEvent::ToolFinished {
        call_id: tool_call_id,
        name: tool_name,
        text: result
          .content
          .iter()
          .filter_map(|content| match content {
            TextContentOrImage::Text(text) => Some(text.text.as_str()),
            _ => None,
          })
          .collect(),
        is_error,
      },
      _ => return,
    };
    (self.0)(mapped);
  }
}
