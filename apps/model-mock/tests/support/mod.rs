#![allow(dead_code)]
use model_mock::{AppConfig, MockApp, Protocol, Reply, Scenario};
use model_provider::ModelConfig;
pub const PROTOCOLS: [Protocol; 3] = [
  Protocol::ChatCompletions,
  Protocol::Responses,
  Protocol::Anthropic,
];
pub async fn app(protocol: Protocol, replies: Vec<Reply>) -> MockApp {
  MockApp::start(settings(protocol, replies)).await.unwrap()
}
pub fn settings(protocol: Protocol, replies: Vec<Reply>) -> AppConfig {
  AppConfig {
    listen: "127.0.0.1:0".parse().unwrap(),
    data_dir: std::env::temp_dir(),
    max_concurrency: 8,
    scenarios: vec![Scenario {
      model: "synthetic".into(),
      protocol,
      repeat_last: false,
      replies,
    }],
  }
}
pub fn text(text: &str) -> Reply {
  Reply::Text {
    text: text.into(),
    delay_ms: 0,
  }
}
pub fn config(app: &MockApp, protocol: Protocol) -> ModelConfig {
  ModelConfig {
    provider: protocol.standard().unwrap().provider().into(),
    model: "synthetic".into(),
    api_key: "synthetic-secret-never-log".into(),
    base_url: Some(app.base_url()),
    system_prompt: None,
    context_window: 16384,
    max_output_tokens: 1024,
    supports_images: false,
  }
}
