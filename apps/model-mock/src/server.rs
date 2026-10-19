use crate::{AppConfig, Protocol, Reply, finite};
use anyhow::{Context, Result};
use axum::{
  Json, Router,
  body::Body,
  extract::{Request, State},
  http::{StatusCode, header},
  response::{IntoResponse, Response},
  routing::get,
};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{
  net::SocketAddr,
  sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
  },
  time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};
use tokio_util::sync::CancellationToken;

struct StateData {
  settings: AppConfig,
  cursors: Vec<AtomicUsize>,
  counts: Vec<AtomicU64>,
  permits: Arc<Semaphore>,
  finite: wiremock::MockServer,
  http: reqwest::Client,
  stop: CancellationToken,
}
pub struct MockApp {
  address: SocketAddr,
  stop: CancellationToken,
  task: Option<JoinHandle<std::io::Result<()>>>,
}
impl MockApp {
  pub async fn start(settings: AppConfig) -> Result<Self> {
    settings.validate()?;
    let listener = TcpListener::bind(settings.listen)
      .await
      .context("bind loopback model mock")?;
    let address = listener.local_addr()?;
    let finite = finite::start(&settings).await?;
    let stop = CancellationToken::new();
    let state = Arc::new(StateData {
      cursors: settings
        .scenarios
        .iter()
        .map(|_| AtomicUsize::new(0))
        .collect(),
      counts: settings
        .scenarios
        .iter()
        .map(|_| AtomicU64::new(0))
        .collect(),
      permits: Arc::new(Semaphore::new(settings.max_concurrency)),
      settings,
      finite,
      stop: stop.clone(),
      http: reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(65))
        .build()?,
    });
    let router = Router::new()
      .route("/health", get(health))
      .route("/v1/models", get(models))
      .route(
        "/api/version",
        get(|| async { Json(json!({"version":"0.0.0-synthetic"})) }),
      )
      .route("/api/tags", get(tags))
      .fallback(respond)
      .with_state(state);
    let shutdown = stop.clone();
    let task = tokio::spawn(async move {
      axum::serve(listener, router)
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
    });
    Ok(Self {
      address,
      stop,
      task: Some(task),
    })
  }
  pub fn address(&self) -> SocketAddr {
    self.address
  }
  pub fn base_url(&self) -> String {
    format!("http://{}", self.address)
  }
  pub async fn shutdown(mut self) {
    self.stop.cancel();
    if let Some(mut task) = self.task.take()
      && tokio::time::timeout(Duration::from_secs(1), &mut task)
        .await
        .is_err()
    {
      task.abort();
      let _ = task.await;
    }
  }
}
impl Drop for MockApp {
  fn drop(&mut self) {
    self.stop.cancel();
    if let Some(task) = self.task.take() {
      task.abort();
    }
  }
}
async fn health(State(state): State<Arc<StateData>>) -> Json<Value> {
  Json(
    json!({"ready":true,"synthetic":true,"scenarios":state.settings.scenarios.iter().enumerate().map(|(index,scenario)|json!({"model":scenario.model,"protocol":scenario.protocol,"requests":state.counts[index].load(Ordering::Relaxed)})).collect::<Vec<_>>()}),
  )
}
async fn models(State(state): State<Arc<StateData>>) -> Json<Value> {
  let mut ids: Vec<_> = state
    .settings
    .scenarios
    .iter()
    .map(|scenario| scenario.model.as_str())
    .collect();
  ids.sort_unstable();
  ids.dedup();
  Json(
    json!({"object":"list","data":ids.into_iter().map(|id|json!({"id":id,"object":"model","owned_by":"synthetic"})).collect::<Vec<_>>()}),
  )
}
async fn tags(State(state): State<Arc<StateData>>) -> Json<Value> {
  Json(crate::ollama::tags(&state.settings))
}
fn failure(status: StatusCode, message: &str) -> Response {
  (
    status,
    Json(json!({"error":{"type":"model_mock","message":message}})),
  )
    .into_response()
}
async fn respond(State(state): State<Arc<StateData>>, request: Request) -> Response {
  if request.method() != axum::http::Method::POST {
    return failure(StatusCode::METHOD_NOT_ALLOWED, "POST required");
  }
  let request_path = request.uri().path().to_owned();
  let protocol = Protocol::from_path(&request_path);
  if protocol.is_none() && request_path != "/api/show" {
    return failure(StatusCode::NOT_FOUND, "unknown synthetic endpoint");
  }
  let Ok(permit) = state.permits.clone().try_acquire_owned() else {
    return failure(StatusCode::TOO_MANY_REQUESTS, "synthetic concurrency limit");
  };
  let bytes = tokio::select! {
    _=state.stop.cancelled()=>return failure(StatusCode::SERVICE_UNAVAILABLE, "synthetic server stopping"),
    result=tokio::time::timeout(Duration::from_secs(10), axum::body::to_bytes(request.into_body(), 1024 * 1024))=>match result {
      Ok(Ok(bytes))=>bytes,
      Ok(Err(_))=>return failure(StatusCode::PAYLOAD_TOO_LARGE, "request exceeds 1 MiB"),
      Err(_)=>return failure(StatusCode::REQUEST_TIMEOUT, "synthetic request timed out"),
    }
  };
  let Ok(body) = serde_json::from_slice::<Value>(&bytes) else {
    return failure(StatusCode::BAD_REQUEST, "JSON request required");
  };
  let Some(model) = body
    .get("model")
    .or_else(|| {
      (request_path == "/api/show")
        .then(|| body.get("name"))
        .flatten()
    })
    .and_then(Value::as_str)
  else {
    return failure(StatusCode::BAD_REQUEST, "model is required");
  };
  if request_path == "/api/show" {
    return crate::ollama::show(&state.settings, model).into_response();
  }
  let Some(protocol) = protocol else {
    return failure(StatusCode::NOT_FOUND, "unknown endpoint");
  };
  let Some(index) = state
    .settings
    .scenarios
    .iter()
    .position(|scenario| scenario.model == model && scenario.protocol == protocol)
  else {
    return if protocol.is_ollama() {
      (
        StatusCode::NOT_FOUND,
        Json(json!({"error":"synthetic model not found"})),
      )
        .into_response()
    } else {
      failure(
        StatusCode::NOT_FOUND,
        "model/protocol scenario is not configured",
      )
    };
  };
  let scenario = &state.settings.scenarios[index];
  let cursor = state.cursors[index]
    .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
      Some(value.saturating_add(1).min(scenario.replies.len()))
    })
    .unwrap_or(0);
  let _ = state.counts[index].try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
    Some(value.saturating_add(1))
  });
  let Some(reply_index) = (cursor < scenario.replies.len())
    .then_some(cursor)
    .or_else(|| scenario.repeat_last.then_some(scenario.replies.len() - 1))
  else {
    return failure(StatusCode::CONFLICT, "synthetic scenario exhausted");
  };
  let reply = &scenario.replies[reply_index];
  let streaming = body
    .get("stream")
    .and_then(Value::as_bool)
    .unwrap_or(protocol.is_ollama());
  // Never retain or log the request body or headers, even on malformed input.
  drop(body);
  drop(bytes);
  tracing::info!(model=%scenario.model, protocol=?protocol, event="synthetic_model_request");
  let response = if streaming || matches!(reply, Reply::Stall | Reply::MissingTerminal { .. }) {
    let response = reply
      .scripted(protocol, &scenario.model, streaming)
      .into_response();
    let (parts, body) = response.into_parts();
    let delay = Duration::from_millis(reply.delay_ms());
    let chunks = futures::stream::once(async move {
      tokio::time::sleep(delay).await;
      body.into_data_stream()
    })
    .flatten();
    Response::from_parts(parts, Body::from_stream(chunks))
  } else {
    match state
      .http
      .get(format!("{}/{index}/{reply_index}", state.finite.uri()))
      .send()
      .await
    {
      Ok(response) => {
        let status = response.status();
        match response.bytes().await {
          Ok(bytes) => {
            (status, [(header::CONTENT_TYPE, "application/json")], bytes).into_response()
          }
          Err(_) => failure(StatusCode::BAD_GATEWAY, "synthetic response unavailable"),
        }
      }
      Err(_) => failure(StatusCode::BAD_GATEWAY, "synthetic response unavailable"),
    }
  };
  // Keep the bounded permit until an SSE consumer finishes or disconnects.
  let (parts, body) = response.into_parts();
  let stream = futures::stream::unfold(
    (body.into_data_stream(), permit, state.stop.clone()),
    |(mut body, permit, stop)| async {
      tokio::select! {
        biased;
        _=stop.cancelled()=>None,
        chunk=body.next()=>chunk.map(|chunk| (chunk, (body, permit, stop))),
      }
    },
  );
  Response::from_parts(parts, Body::from_stream(stream))
}
