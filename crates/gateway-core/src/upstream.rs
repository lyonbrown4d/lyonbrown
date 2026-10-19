//! Host resource policy around Rig's public raw HTTP transport.
use crate::Secret;
use axum::http::{Extensions, header};
use rig_core::{
  http_client::{Error, HttpClientExt, Request, StreamingResponse, make_auth_header},
  wasm_compat::WasmBoxedFuture,
};
use rig_reqwest::{ReqwestMiddlewareClient, reqwest};

pub(crate) struct Client(ReqwestMiddlewareClient);

impl Client {
  pub fn new() -> Result<Self, reqwest::Error> {
    let client = reqwest::Client::builder()
      .redirect(reqwest::redirect::Policy::none())
      .retry(reqwest::retry::never())
      .no_proxy()
      .build()?;
    Ok(Self(ReqwestMiddlewareClient::from(
      reqwest_middleware::ClientBuilder::new(client)
        .with(discard_error_body)
        .build(),
    )))
  }

  /// Both JSON and SSE stay streamed so the host enforces its own byte/deadline limits.
  pub async fn send(
    &self,
    base: &str,
    key: Option<&Secret>,
    request_id: &str,
    body: &serde_json::Value,
  ) -> Result<StreamingResponse, Box<Error>> {
    let mut request = Request::post(format!("{}/chat/completions", base.trim_end_matches('/')))
      .header(header::CONTENT_TYPE, "application/json")
      .header("x-request-id", request_id);
    if let Some(key) = key {
      let (name, mut authorization) = make_auth_header(key.expose_secret())?;
      authorization.set_sensitive(true);
      request = request.header(name, authorization);
    }
    let bytes = serde_json::to_vec(body).map_err(Error::instance)?;
    self
      .0
      .send_streaming(request.body(bytes).map_err(Error::from)?)
      .await
      .map_err(Box::new)
  }
}

/// Rig reads rejected response bodies before returning an HTTP error. Drop that
/// untrusted stream here, before Rig can buffer or expose provider content.
fn discard_error_body<'a>(
  request: reqwest::Request,
  extensions: &'a mut Extensions,
  next: reqwest_middleware::Next<'a>,
) -> WasmBoxedFuture<'a, reqwest_middleware::Result<reqwest::Response>> {
  Box::pin(async move {
    let response = next.run(request, extensions).await?;
    if response.status().is_success() {
      return Ok(response);
    }
    let mut empty = axum::http::Response::new(Vec::<u8>::new());
    *empty.status_mut() = response.status();
    *empty.version_mut() = response.version();
    if let Some(retry_after) = response.headers().get(header::RETRY_AFTER) {
      empty
        .headers_mut()
        .insert(header::RETRY_AFTER, retry_after.clone());
    }
    drop(response);
    Ok(reqwest::Response::from(empty))
  })
}
