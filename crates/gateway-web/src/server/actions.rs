use super::{
  auth,
  errors::{WebError, redirect},
};
use axum::http::{HeaderValue, StatusCode, header};
use gateway_validation::Credentials;
use leptos::prelude::*;
use leptos_axum::ResponseOptions;

pub(crate) async fn login(username: &str, password: &str) -> Result<(), ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  if !auth::origin_valid(&state, &headers) {
    return Err(WebError::forbidden().server_fn());
  }
  let invalid = || {
    WebError {
      status: StatusCode::UNAUTHORIZED,
      message: "用户名或密码无效",
    }
    .server_fn()
  };
  if (Credentials { username, password }).validate().is_err() {
    return Err(invalid());
  }
  let timestamp = auth::now().map_err(WebError::server_fn)?;
  let session = state
    .store
    .login(username, password, timestamp, 3600)
    .await
    .map_err(|_| invalid())?;
  let options =
    use_context::<ResponseOptions>().ok_or_else(|| WebError::unavailable().server_fn())?;
  for (name, value) in [
    (auth::SESSION, session.token()),
    (auth::CSRF, session.csrf_token()),
  ] {
    let cookie = HeaderValue::from_str(&auth::cookie(name, value, 3600, state.secure))
      .map_err(|_| WebError::unavailable().server_fn())?;
    options.append_header(header::SET_COOKIE, cookie);
  }
  redirect("/ui/models", &headers);
  Ok(())
}
pub(crate) async fn logout(csrf: &str) -> Result<(), ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  let token = auth::verify_post(&state, &headers, csrf)
    .await
    .map_err(WebError::server_fn)?;
  state
    .store
    .revoke_session(&token)
    .await
    .map_err(|_| WebError::unavailable().server_fn())?;
  let options =
    use_context::<ResponseOptions>().ok_or_else(|| WebError::unavailable().server_fn())?;
  for name in [auth::SESSION, auth::CSRF] {
    let cookie = HeaderValue::from_str(&auth::cookie(name, "", 0, state.secure))
      .map_err(|_| WebError::unavailable().server_fn())?;
    options.append_header(header::SET_COOKIE, cookie);
  }
  redirect("/ui/login", &headers);
  Ok(())
}
