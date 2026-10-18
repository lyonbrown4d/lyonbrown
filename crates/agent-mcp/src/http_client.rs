//! Bounded HTTP adapter through the SDK's public transport extension point.
use std::{collections::HashMap, sync::Arc};

use futures::{StreamExt, TryStreamExt, stream::BoxStream};
use http::{HeaderName, HeaderValue};
use rmcp::{
  model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
  transport::streamable_http_client::{
    StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
  },
};
use sse_stream::{Sse, SseStream};

#[derive(Clone)]
pub(crate) struct BoundedHttp {
  pub client: reqwest::Client,
  pub max_bytes: usize,
}

type HttpError = StreamableHttpError<reqwest::Error>;

fn rejected() -> HttpError {
  HttpError::UnexpectedServerResponse("MCP HTTP response rejected".into())
}

impl StreamableHttpClient for BoundedHttp {
  type Error = reqwest::Error;

  async fn post_message(
    &self,
    uri: Arc<str>,
    message: ClientJsonRpcMessage,
    session_id: Option<Arc<str>>,
    auth_header: Option<String>,
    custom_headers: HashMap<HeaderName, HeaderValue>,
  ) -> std::result::Result<StreamableHttpPostResponse, HttpError> {
    let mut request = self
      .client
      .post(uri.as_ref())
      .header("accept", "application/json, text/event-stream");
    for (key, value) in custom_headers {
      request = request.header(key, value);
    }
    if let Some(session_id) = session_id.as_ref() {
      request = request.header("mcp-session-id", session_id.as_ref());
    }
    if let Some(token) = auth_header {
      request = request.bearer_auth(token);
    }
    let mut response = request.json(&message).send().await?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND && session_id.is_some() {
      return Err(HttpError::SessionExpired);
    }
    if !status.is_success() {
      return Err(rejected());
    }
    if matches!(status.as_u16(), 202 | 204) {
      return Ok(StreamableHttpPostResponse::Accepted);
    }
    let session = response
      .headers()
      .get("mcp-session-id")
      .and_then(|value| value.to_str().ok())
      .map(str::to_owned);
    let content_type = response
      .headers()
      .get("content-type")
      .and_then(|value| value.to_str().ok())
      .unwrap_or_default()
      .to_owned();
    if content_type.starts_with("text/event-stream") {
      // Each response has a total raw-byte budget before SSE/JSON parsing.
      // This intentionally also bounds long-lived POST streams.
      let mut remaining = self.max_bytes;
      let bytes = response
        .bytes_stream()
        .map_err(std::io::Error::other)
        .and_then(move |chunk| {
          let valid = chunk.len() <= remaining;
          remaining = remaining.saturating_sub(chunk.len());
          std::future::ready(if valid {
            Ok(chunk)
          } else {
            Err(std::io::Error::other("MCP response byte limit exceeded"))
          })
        });
      return Ok(StreamableHttpPostResponse::Sse(
        SseStream::from_bytes_stream(bytes).boxed(),
        session,
      ));
    }
    if !content_type.starts_with("application/json") {
      return Err(rejected());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
      if chunk.len() > self.max_bytes.saturating_sub(body.len()) {
        return Err(rejected());
      }
      body.extend_from_slice(&chunk);
    }
    if body.is_empty() && !matches!(message, ClientJsonRpcMessage::Request(_)) {
      return Ok(StreamableHttpPostResponse::Accepted);
    }
    let message: ServerJsonRpcMessage = serde_json::from_slice(&body)?;
    Ok(StreamableHttpPostResponse::Json(message, session))
  }

  async fn delete_session(
    &self,
    uri: Arc<str>,
    session_id: Arc<str>,
    auth_header: Option<String>,
    custom_headers: HashMap<HeaderName, HeaderValue>,
  ) -> std::result::Result<(), HttpError> {
    self
      .client
      .delete_session(uri, session_id, auth_header, custom_headers)
      .await
  }

  async fn get_stream(
    &self,
    uri: Arc<str>,
    session_id: Option<Arc<str>>,
    last_event_id: Option<String>,
    auth_header: Option<String>,
    custom_headers: HashMap<HeaderName, HeaderValue>,
  ) -> std::result::Result<BoxStream<'static, std::result::Result<Sse, sse_stream::Error>>, HttpError>
  {
    self
      .client
      .get_stream_with_max_sse_event_size(
        uri,
        session_id,
        last_event_id,
        auth_header,
        custom_headers,
        self.max_bytes,
      )
      .await
  }

  async fn get_stream_with_max_sse_event_size(
    &self,
    uri: Arc<str>,
    session_id: Option<Arc<str>>,
    last_event_id: Option<String>,
    auth_header: Option<String>,
    custom_headers: HashMap<HeaderName, HeaderValue>,
    max_size: usize,
  ) -> std::result::Result<BoxStream<'static, std::result::Result<Sse, sse_stream::Error>>, HttpError>
  {
    self
      .client
      .get_stream_with_max_sse_event_size(
        uri,
        session_id,
        last_event_id,
        auth_header,
        custom_headers,
        max_size.min(self.max_bytes),
      )
      .await
  }
}
