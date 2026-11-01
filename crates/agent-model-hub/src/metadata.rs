use anyhow::{Result, ensure};
use std::time::Duration;

pub(crate) fn client(token: Option<&str>) -> Result<reqwest::Client> {
  let mut headers = reqwest::header::HeaderMap::new();
  if let Some(token) = token.filter(|value| !value.is_empty()) {
    let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))?;
    value.set_sensitive(true);
    headers.insert(reqwest::header::AUTHORIZATION, value);
  }
  Ok(
    reqwest::Client::builder()
      .default_headers(headers)
      .redirect(reqwest::redirect::Policy::none())
      .timeout(Duration::from_secs(10))
      .build()?,
  )
}

impl crate::HubClient {
  pub(crate) async fn artifact_hash(
    &self,
    repository: &str,
    revision: &str,
    path: &str,
    size: u64,
  ) -> Result<String> {
    // hf-hub 1.0's public HEAD API follows CDN redirects and can lose linked
    // metadata. This transport adapter reads only the original Hub headers.
    let mut url = self.endpoint.clone();
    {
      let mut segments = url
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid Hub endpoint"))?;
      segments.pop_if_empty();
      for part in repository.split('/') {
        segments.push(part);
      }
      segments.push("resolve").push(revision);
      for part in path.split('/') {
        segments.push(part);
      }
    }
    let response = self
      .metadata
      .head(url)
      .send()
      .await
      .map_err(|_| anyhow::anyhow!("artifact metadata unavailable"))?;
    ensure!(
      response.status().is_success() || response.status().is_redirection(),
      "artifact metadata request failed"
    );
    let headers = response.headers();
    if let Some(commit) = headers.get("x-repo-commit") {
      ensure!(commit.to_str()? == revision, "artifact revision changed");
    }
    let length = headers
      .get("x-linked-size")
      .or_else(|| headers.get("content-length"))
      .ok_or_else(|| anyhow::anyhow!("artifact size missing"))?
      .to_str()?
      .parse::<u64>()?;
    ensure!(length == size, "artifact size changed");
    let hash = headers
      .get("x-linked-etag")
      .or_else(|| headers.get("etag"))
      .ok_or_else(|| anyhow::anyhow!("artifact digest missing"))?
      .to_str()?
      .trim_matches('"');
    ensure!(
      hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
      "LFS artifact has no SHA256 digest"
    );
    Ok(hash.to_owned())
  }
}
