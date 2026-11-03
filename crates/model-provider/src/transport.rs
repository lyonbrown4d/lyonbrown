//! Host limits around the framework transport; protocol encoding belongs to Rig.
use crate::{ProviderError, Result};
use futures::StreamExt;
use rig_core::http_client::DynHttpClient;
use rig_reqwest::{ReqwestMiddlewareClient, reqwest};
use std::time::Duration;

pub fn configured_http(
  base_url: &str,
  timeout: Duration,
  body_limit: usize,
) -> Result<DynHttpClient> {
  agent_security::initialize_tls();
  let url = reqwest::Url::parse(base_url).map_err(|_| invalid())?;
  let loopback = match url.host() {
    Some(url::Host::Domain("localhost")) => true,
    Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
    Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
    _ => false,
  };
  if !(1..=64 * 1024 * 1024).contains(&body_limit)
    || timeout.is_zero()
    || url.host().is_none()
    || !(url.scheme() == "https" || url.scheme() == "http" && loopback)
    || !url.username().is_empty()
    || url.password().is_some()
    || url.query().is_some()
    || url.fragment().is_some()
  {
    return Err(invalid());
  }
  let builder = reqwest::Client::builder()
    .connect_timeout(timeout.min(Duration::from_secs(15)))
    .timeout(timeout)
    .redirect(reqwest::redirect::Policy::none())
    .retry(reqwest::retry::never());
  let client = if loopback {
    builder.no_proxy()
  } else {
    builder
  }
  .build()
  .map_err(|_| invalid())?;
  let client = reqwest_middleware::ClientBuilder::new(client)
    .with(ResponseLimit { bytes: body_limit })
    .build();
  Ok(DynHttpClient::new(ReqwestMiddlewareClient::from(client)))
}

struct ResponseLimit {
  bytes: usize,
}
#[async_trait::async_trait]
impl reqwest_middleware::Middleware for ResponseLimit {
  async fn handle(
    &self,
    request: reqwest::Request,
    extensions: &mut http::Extensions,
    next: reqwest_middleware::Next<'_>,
  ) -> reqwest_middleware::Result<reqwest::Response> {
    let response = next.run(request, extensions).await?;
    let status = response.status();
    let version = response.version();
    let mut headers = response.headers().clone();
    headers.remove(http::header::CONTENT_LENGTH);
    let body = if status.is_success() {
      let limit = self.bytes;
      let chunks = response.bytes_stream();
      let bounded =
        futures::stream::try_unfold((chunks, 0_usize), move |(mut chunks, total)| async move {
          let Some(chunk) = chunks.next().await else {
            return Ok(None);
          };
          let chunk = chunk.map_err(|_| std::io::Error::other("model response unavailable"))?;
          let next = total
            .checked_add(chunk.len())
            .filter(|size| *size <= limit)
            .ok_or_else(|| std::io::Error::other("model response exceeds host limit"))?;
          Ok::<_, std::io::Error>(Some((chunk, (chunks, next))))
        });
      reqwest::Body::wrap_stream(bounded)
    } else {
      // Rig reads rejected bodies. Discard them before it can buffer an
      // untrusted body or echo a credential-bearing provider error.
      drop(response);
      reqwest::Body::from(Vec::<u8>::new())
    };
    let mut response = http::Response::new(body);
    *response.status_mut() = status;
    *response.version_mut() = version;
    *response.headers_mut() = headers;
    Ok(reqwest::Response::from(response))
  }
}

fn invalid() -> ProviderError {
  ProviderError::Configuration("invalid model HTTP transport configuration".into())
}
