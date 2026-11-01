//! Canonical tool history through the SDK's public RequestLike extension port.
use std::{collections::BTreeMap, sync::Arc};

use either::Either;
use indexmap::IndexMap;
use mistralrs::{MessageContent, RequestBuilder, RequestLike, RequestMessage};
use serde_json::json;

use crate::ToolCall;

pub(crate) struct NativeRequest {
  builder: RequestBuilder,
  messages: Vec<IndexMap<String, MessageContent>>,
  tool_history: BTreeMap<usize, Vec<ToolCall>>,
}

impl NativeRequest {
  pub(crate) fn new(builder: RequestBuilder, tool_history: BTreeMap<usize, Vec<ToolCall>>) -> Self {
    let mut request = Self {
      builder,
      messages: Vec::new(),
      tool_history,
    };
    request.refresh_messages();
    request
  }

  fn refresh_messages(&mut self) {
    self.messages = self.builder.messages_ref().to_vec();
    for (index, message) in self.messages.iter_mut().enumerate() {
      if let Some(calls) = self.tool_history.get(&index) {
        let calls = calls
          .iter()
          .map(|call| {
            IndexMap::from([
              ("id".into(), json!(call.id)),
              ("type".into(), json!("function")),
              (
                "function".into(),
                json!({"name":call.name,"arguments":call.arguments}),
              ),
            ])
          })
          .collect();
        message.insert("tool_calls".into(), Either::Right(calls));
      }
    }
  }
}

impl RequestLike for NativeRequest {
  fn messages_ref(&self) -> &[IndexMap<String, MessageContent>] {
    &self.messages
  }
  fn images_ref(&self) -> &[image::DynamicImage] {
    self.builder.images_ref()
  }
  fn take_messages(&mut self) -> RequestMessage {
    let mut request = self.builder.take_messages();
    match &mut request {
      RequestMessage::Chat { messages, .. } | RequestMessage::MultimodalChat { messages, .. } => {
        *messages = std::mem::take(&mut self.messages);
      }
      _ => {}
    }
    request
  }
  fn take_logits_processors(&mut self) -> Option<Vec<Arc<dyn mistralrs::CustomLogitsProcessor>>> {
    self.builder.take_logits_processors()
  }
  fn take_adapters(&mut self) -> Option<Vec<String>> {
    self.builder.take_adapters()
  }
  fn return_logprobs(&self) -> bool {
    RequestLike::return_logprobs(&self.builder)
  }
  fn enable_search(&self) -> Option<bool> {
    self.builder.enable_search()
  }
  fn take_constraint(&mut self) -> mistralrs::Constraint {
    self.builder.take_constraint()
  }
  fn take_tools(&mut self) -> Option<(Vec<mistralrs::Tool>, mistralrs::ToolChoice)> {
    self.builder.take_tools()
  }
  fn take_sampling_params(&mut self) -> mistralrs::SamplingParams {
    self.builder.take_sampling_params()
  }
  fn take_web_search_options(&mut self) -> Option<mistralrs::WebSearchOptions> {
    self.builder.take_web_search_options()
  }
  fn truncate_sequence(&self) -> bool {
    self.builder.truncate_sequence()
  }
  fn resolve_pending_prefixes(&mut self, category: &mistralrs::ModelCategory) {
    self.builder.resolve_pending_prefixes(category);
    self.refresh_messages();
  }
}
