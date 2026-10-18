use agent_inference::InferenceChunk;
use rpi_ai::event_stream::AssistantMessageEventStreamProducer;
use rpi_ai::{
  AssistantMessage, AssistantMessageEvent as Event, Content, DoneReason, ErrorReason, Model,
  StopReason, Usage,
};
use std::sync::Arc;
pub(crate) struct StreamState {
  message: AssistantMessage,
  producer: AssistantMessageEventStreamProducer,
  open: Option<usize>,
}
impl StreamState {
  pub(crate) fn new(model: Model, mut producer: AssistantMessageEventStreamProducer) -> Self {
    let message = AssistantMessage {
      role: Default::default(),
      content: Vec::new(),
      api: model.api,
      provider: model.provider,
      model: model.id,
      response_model: None,
      response_id: None,
      usage: Usage::zero(),
      stop_reason: StopReason::Pending,
      deferred: None,
      error_message: None,
      raw_stop_reason: None,
      end_turn: None,
      timestamp: rpi_harness::session::memory::Clock::now_ms(
        &rpi_harness::session::memory::SystemClock,
      ),
    };
    producer.push(Event::Start {
      partial: Arc::new(message.clone()),
    });
    Self {
      message,
      producer,
      open: None,
    }
  }
  fn close(&mut self) {
    if let Some(index) = self.open.take() {
      let partial = Arc::new(self.message.clone());
      match &self.message.content[index] {
        Content::Text(text) => {
          self.producer.push(Event::TextEnd {
            content_index: index,
            content: text.text.clone(),
            partial,
          });
        }
        Content::Thinking(thinking) => {
          self.producer.push(Event::ThinkingEnd {
            content_index: index,
            content: thinking.thinking.clone(),
            partial,
          });
        }
        _ => {}
      }
    }
  }
  pub(crate) fn chunk(&mut self, chunk: InferenceChunk) -> bool {
    match chunk {
      InferenceChunk::Text { text } => self.delta(text, false),
      InferenceChunk::Thinking { text } => self.delta(text, true),
      InferenceChunk::ToolCall { call } => {
        self.close();
        let index = self.message.content.len();
        self.message.content.push(Content::tool_call(
          &call.id,
          &call.name,
          serde_json::json!({}),
        ));
        self.producer.push(Event::ToolCallStart {
          content_index: index,
          partial: Arc::new(self.message.clone()),
        });
        let tool = rpi_ai::ToolCall {
          kind: Default::default(),
          id: call.id,
          name: call.name,
          arguments: call.arguments,
          thought_signature: None,
          namespace: None,
        };
        self.message.content[index] = Content::ToolCall(tool.clone());
        self.producer.push(Event::ToolCallDelta {
          content_index: index,
          delta: tool.arguments.to_string(),
          partial: Arc::new(self.message.clone()),
        });
        self.producer.push(Event::ToolCallEnd {
          content_index: index,
          tool_call: tool,
          partial: Arc::new(self.message.clone()),
        })
      }
    }
  }
  fn delta(&mut self, text: String, thinking: bool) -> bool {
    let compatible = self.open.is_some_and(|index| {
      matches!(
        (&self.message.content[index], thinking),
        (Content::Text(_), false) | (Content::Thinking(_), true)
      )
    });
    if !compatible {
      self.close();
      let index = self.message.content.len();
      self.open = Some(index);
      if thinking {
        self
          .message
          .content
          .push(Content::Thinking(rpi_ai::ThinkingContent {
            kind: Default::default(),
            thinking: String::new(),
            thinking_signature: None,
            redacted: false,
          }));
        self.producer.push(Event::ThinkingStart {
          content_index: index,
          partial: Arc::new(self.message.clone()),
        });
      } else {
        self.message.content.push(Content::text(""));
        self.producer.push(Event::TextStart {
          content_index: index,
          partial: Arc::new(self.message.clone()),
        });
      }
    }
    let Some(index) = self.open else {
      return false;
    };
    if thinking {
      if let Content::Thinking(content) = &mut self.message.content[index] {
        content.thinking.push_str(&text);
      }
      self.producer.push(Event::ThinkingDelta {
        content_index: index,
        delta: text,
        partial: Arc::new(self.message.clone()),
      })
    } else {
      if let Content::Text(content) = &mut self.message.content[index] {
        content.text.push_str(&text);
      }
      self.producer.push(Event::TextDelta {
        content_index: index,
        delta: text,
        partial: Arc::new(self.message.clone()),
      })
    }
  }
  pub(crate) fn finish(&mut self, summary: agent_inference::GenerationSummary) {
    self.close();
    self.message.usage.input = i64::try_from(summary.input_tokens).unwrap_or(i64::MAX);
    self.message.usage.output = i64::try_from(summary.output_tokens).unwrap_or(i64::MAX);
    self.message.usage.total_tokens = self
      .message
      .usage
      .input
      .saturating_add(self.message.usage.output);
    let reason = match summary.reason {
      agent_inference::FinishReason::Stop => DoneReason::Stop,
      agent_inference::FinishReason::Length => DoneReason::Length,
      agent_inference::FinishReason::ToolCalls => DoneReason::ToolUse,
    };
    self.message.stop_reason = match summary.reason {
      agent_inference::FinishReason::Stop => StopReason::Stop,
      agent_inference::FinishReason::Length => StopReason::Length,
      agent_inference::FinishReason::ToolCalls => StopReason::ToolUse,
    };
    self.producer.push(Event::Done {
      reason,
      message: self.message.clone(),
    });
  }
  pub(crate) fn fail(&mut self, error: String, cancelled: bool) {
    self.close();
    self.message.stop_reason = if cancelled {
      StopReason::Aborted
    } else {
      StopReason::Error
    };
    self.message.error_message = Some(error);
    self.producer.push(Event::Error {
      reason: if cancelled {
        ErrorReason::Aborted
      } else {
        ErrorReason::Error
      },
      error: self.message.clone(),
    });
  }
}
