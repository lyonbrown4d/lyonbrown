use axum::{
  Router,
  body::{Body, Bytes},
  extract::{Request, State},
  http::{HeaderName, HeaderValue, StatusCode},
  response::Response,
};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{
  collections::{BTreeMap, VecDeque},
  convert::Infallible,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
};
use tokio::{
  net::TcpListener,
  sync::{Mutex, Notify},
  task::JoinHandle,
};

#[derive(Clone)]
pub struct RecordedRequest {
  pub method: String,
  pub path: String,
  pub headers: BTreeMap<String, String>,
  pub body: Value,
}
#[derive(Clone)]
pub struct ScriptedReply {
  status: u16,
  headers: Vec<(String, String)>,
  chunks: Vec<Vec<u8>>,
  stall: bool,
}
impl ScriptedReply {
  pub fn sse(events: Vec<Value>) -> Self {
    Self::raw_sse(
      events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>()
        .into_bytes(),
    )
  }
  pub fn named_sse(events: Vec<(String, Value)>) -> Self {
    Self::raw_sse(
      events
        .into_iter()
        .map(|(name, event)| format!("event: {name}\ndata: {event}\n\n"))
        .collect::<String>()
        .into_bytes(),
    )
  }
  pub fn raw_sse(bytes: Vec<u8>) -> Self {
    Self {
      status: 200,
      headers: vec![("content-type".into(), "text/event-stream".into())],
      chunks: vec![bytes],
      stall: false,
    }
  }
  pub fn fragmented(self, bytes_per_chunk: usize) -> Self {
    let bytes = self.chunks.concat();
    Self {
      chunks: bytes
        .chunks(bytes_per_chunk.max(1))
        .map(<[u8]>::to_vec)
        .collect(),
      ..self
    }
  }
  pub fn status(status: u16, body: Value) -> Self {
    Self {
      status,
      headers: vec![("content-type".into(), "application/json".into())],
      chunks: vec![body.to_string().into_bytes()],
      stall: false,
    }
  }
  pub fn stall() -> Self {
    Self {
      status: 200,
      headers: vec![("content-type".into(), "text/event-stream".into())],
      chunks: Vec::new(),
      stall: true,
    }
  }
  /// Emit the scripted prefix and leave the response open until the caller cancels.
  pub fn stall_after(mut self) -> Self {
    self.stall = true;
    self
  }
  pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
    self.headers.push((name.into(), value.into()));
    self
  }
}
struct ServerState {
  replies: Mutex<VecDeque<ScriptedReply>>,
  requests: Mutex<Vec<RecordedRequest>>,
  changed: Notify,
  replies_changed: Notify,
  interactive: bool,
  closed: AtomicBool,
}
pub struct MockModelServer {
  url: String,
  state: Arc<ServerState>,
  task: JoinHandle<()>,
}
impl MockModelServer {
  pub async fn start(replies: Vec<ScriptedReply>) -> std::io::Result<Self> {
    Self::start_mode(replies, false).await
  }
  /// A test can release the next model step after observing a durable external state.
  pub async fn start_interactive() -> std::io::Result<Self> {
    Self::start_mode(Vec::new(), true).await
  }
  async fn start_mode(replies: Vec<ScriptedReply>, interactive: bool) -> std::io::Result<Self> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", listener.local_addr()?);
    let state = Arc::new(ServerState {
      replies: Mutex::new(replies.into()),
      requests: Mutex::new(Vec::new()),
      changed: Notify::new(),
      replies_changed: Notify::new(),
      interactive,
      closed: AtomicBool::new(false),
    });
    let router = Router::new().fallback(respond).with_state(state.clone());
    let task = tokio::spawn(async move {
      let _ = axum::serve(listener, router).await;
    });
    Ok(Self { url, state, task })
  }
  pub fn base_url(&self) -> &str {
    &self.url
  }
  pub async fn push_reply(&self, reply: ScriptedReply) {
    self.state.replies.lock().await.push_back(reply);
    self.state.replies_changed.notify_one();
  }
  pub async fn requests(&self) -> Vec<RecordedRequest> {
    self.state.requests.lock().await.clone()
  }
  pub async fn wait_for_requests(&self, count: usize) {
    loop {
      let changed = self.state.changed.notified();
      tokio::pin!(changed);
      changed.as_mut().enable();
      if self.state.requests.lock().await.len() >= count {
        return;
      }
      changed.await;
    }
  }
}
impl Drop for MockModelServer {
  fn drop(&mut self) {
    self.state.closed.store(true, Ordering::Release);
    self.state.replies_changed.notify_waiters();
    self.task.abort();
  }
}
async fn respond(State(state): State<Arc<ServerState>>, request: Request) -> Response {
  let (parts, body) = request.into_parts();
  let bytes = match axum::body::to_bytes(body, 32 * 1024 * 1024).await {
    Ok(bytes) => bytes,
    Err(_) => {
      return Response::builder()
        .status(StatusCode::PAYLOAD_TOO_LARGE)
        .body(Body::empty())
        .unwrap_or_default();
    }
  };
  let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
  let headers = parts
    .headers
    .iter()
    .filter_map(|(name, value)| {
      value
        .to_str()
        .ok()
        .map(|value| (name.to_string(), value.into()))
    })
    .collect();
  state.requests.lock().await.push(RecordedRequest {
    method: parts.method.to_string(),
    path: parts.uri.path().into(),
    headers,
    body,
  });
  state.changed.notify_waiters();
  let reply = loop {
    let changed = state.replies_changed.notified();
    tokio::pin!(changed);
    changed.as_mut().enable();
    if let Some(reply) = state.replies.lock().await.pop_front() {
      break reply;
    }
    if !state.interactive {
      break ScriptedReply::status(
        500,
        serde_json::json!({"error":{"message":"no scripted response"}}),
      );
    }
    if state.closed.load(Ordering::Acquire) {
      break ScriptedReply::status(
        503,
        serde_json::json!({"error":{"message":"fixture closed"}}),
      );
    }
    changed.await;
  };
  let chunks = stream::iter(
    reply
      .chunks
      .into_iter()
      .map(|bytes| Ok::<_, Infallible>(Bytes::from(bytes))),
  );
  let body = if reply.stall {
    Body::from_stream(chunks.chain(stream::pending()))
  } else {
    Body::from_stream(chunks)
  };
  let mut response = Response::new(body);
  *response.status_mut() =
    StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
  for (name, value) in reply.headers {
    if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
      response.headers_mut().insert(name, value);
    }
  }
  response
}
