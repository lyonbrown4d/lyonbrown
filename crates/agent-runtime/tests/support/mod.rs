use agent_runtime::{Result, RuntimeConfig, RuntimeError, SessionMutation, SessionPersistence};
use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
#[derive(Default)]
pub struct Memory {
  pub snapshot: Mutex<Option<Vec<u8>>>,
  pub mutations: Mutex<Vec<SessionMutation>>,
}
#[async_trait::async_trait]
impl SessionPersistence for Memory {
  async fn load(&self, _: &str) -> Result<Option<Vec<u8>>> {
    Ok(self.snapshot.lock().unwrap().clone())
  }
  async fn commit(
    &self,
    _: &str,
    epoch: u64,
    snapshot: Vec<u8>,
    mutation: SessionMutation,
  ) -> Result<()> {
    if epoch != 1 {
      return Err(RuntimeError::Persistence("stale owner".into()));
    }
    *self.snapshot.lock().unwrap() = Some(snapshot);
    self.mutations.lock().unwrap().push(mutation);
    Ok(())
  }
}
pub type Reply = ScriptedReply;
pub struct Server {
  pub endpoint: String,
  server: MockModelServer,
  next: usize,
}
impl Server {
  pub async fn start(replies: Vec<Reply>) -> Self {
    let server = MockModelServer::start(replies).await.unwrap();
    Self {
      endpoint: server.base_url().into(),
      server,
      next: 0,
    }
  }
  pub async fn request(&mut self) -> Value {
    self.next += 1;
    self.server.wait_for_requests(self.next).await;
    self.server.requests().await[self.next - 1].body.clone()
  }
}
pub fn config(endpoint: &str) -> RuntimeConfig {
  RuntimeConfig {
    session_id: "session".into(),
    owner_epoch: 1,
    provider: "openai-completions".into(),
    model: "fixture-model".into(),
    api_key: "fixture-secret-never-persist".into(),
    base_url: Some(endpoint.into()),
    system_prompt: Some("Use supplied tools.".into()),
    context_window: 128_000,
    max_output_tokens: 4096,
    supports_images: true,
  }
}
pub fn text(text: &str) -> Reply {
  text_reply(StandardProtocol::ChatCompletions, text)
}
pub fn tool() -> Reply {
  tool_reply(
    StandardProtocol::ChatCompletions,
    "inspect",
    serde_json::json!({"path":"file.txt"}),
  )
}
pub fn shared() -> Arc<Memory> {
  Arc::new(Memory::default())
}
