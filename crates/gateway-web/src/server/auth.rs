use super::{WebState, errors::WebError};
use axum::http::{HeaderMap, header, request::Parts};
use gateway_storage::AdminRole;
use leptos::prelude::*;
use std::time::{SystemTime, UNIX_EPOCH};
pub(crate) const SESSION: &str = "lyonbrown_admin";
pub(crate) const CSRF: &str = "lyonbrown_csrf";
pub(crate) fn now() -> Result<u64, WebError> {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|v| v.as_secs())
    .map_err(|_| WebError::unavailable())
}
pub(crate) fn context() -> Result<(WebState, HeaderMap), WebError> {
  Ok((
    use_context::<WebState>().ok_or_else(WebError::unavailable)?,
    use_context::<Parts>()
      .ok_or_else(WebError::unavailable)?
      .headers,
  ))
}
pub(crate) fn origin_valid(state: &WebState, headers: &HeaderMap) -> bool {
  state.origin.as_ref().is_some_and(|expected| {
    headers
      .get(header::ORIGIN)
      .and_then(|v| v.to_str().ok())
      .is_some_and(|actual| actual == expected)
  })
}
fn cookies(headers: &HeaderMap) -> Result<(String, String), WebError> {
  let mut token = None;
  let mut csrf = None;
  for value in headers.get_all(header::COOKIE) {
    let value = value.to_str().map_err(|_| WebError::unauthorized())?;
    for pair in value.split(';') {
      let Some((name, value)) = pair.trim().split_once('=') else {
        continue;
      };
      let target = match name {
        SESSION => &mut token,
        CSRF => &mut csrf,
        _ => continue,
      };
      if target.is_some() || value.len() != 64 || !value.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err(WebError::unauthorized());
      }
      *target = Some(value.to_owned());
    }
  }
  token.zip(csrf).ok_or_else(WebError::unauthorized)
}
pub(crate) async fn authenticated(
  state: &WebState,
  headers: &HeaderMap,
) -> Result<(String, String), WebError> {
  let (token, csrf) = cookies(headers)?;
  let timestamp = now()?;
  match state.store.session_identity(&token, timestamp).await {
    Ok(Some(identity)) if identity.role == AdminRole::SuperAdmin => {}
    Ok(_) => return Err(WebError::unauthorized()),
    Err(_) => return Err(WebError::unavailable()),
  }
  match state.store.verify_csrf(&token, &csrf, timestamp).await {
    Ok(true) => Ok((token, csrf)),
    Ok(false) => Err(WebError::unauthorized()),
    Err(_) => Err(WebError::unavailable()),
  }
}
pub(crate) async fn verify_post(
  state: &WebState,
  headers: &HeaderMap,
  csrf: &str,
) -> Result<String, WebError> {
  if !origin_valid(state, headers) {
    return Err(WebError::forbidden());
  }
  let (token, _) = authenticated(state, headers).await?;
  match state.store.verify_csrf(&token, csrf, now()?).await {
    Ok(true) => Ok(token),
    Ok(false) => Err(WebError::forbidden()),
    Err(_) => Err(WebError::unavailable()),
  }
}
pub(crate) fn cookie(name: &str, value: &str, max_age: u32, secure: bool) -> String {
  format!(
    "{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{}",
    if secure { "; Secure" } else { "" }
  )
}
