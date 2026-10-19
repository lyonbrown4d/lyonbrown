use axum::{
  http::{HeaderMap, HeaderValue, StatusCode, header},
  response::{IntoResponse, Response},
};
use leptos::prelude::*;
use leptos_axum::ResponseOptions;
#[derive(Clone, Copy)]
pub(crate) struct WebError {
  pub status: StatusCode,
  pub message: &'static str,
}
impl WebError {
  pub fn unavailable() -> Self {
    Self {
      status: StatusCode::SERVICE_UNAVAILABLE,
      message: "服务暂时不可用，请稍后重试。",
    }
  }
  pub fn unauthorized() -> Self {
    Self {
      status: StatusCode::UNAUTHORIZED,
      message: "请先登录管理台。",
    }
  }
  pub fn forbidden() -> Self {
    Self {
      status: StatusCode::FORBIDDEN,
      message: "请求来源无效或表单已失效，请刷新页面重试。",
    }
  }
  pub fn server_fn(self) -> ServerFnError {
    if let Some(options) = use_context::<ResponseOptions>() {
      options.set_status(self.status);
    }
    ServerFnError::new(self.message)
  }
}
impl IntoResponse for WebError {
  fn into_response(self) -> Response {
    (self.status, self.message).into_response()
  }
}
pub(crate) fn redirect(path: &'static str, headers: &HeaderMap) {
  leptos_axum::redirect(path);
  // Preserve native HTML form semantics; enhanced fetches use Leptos's redirect header.
  if headers
    .get(header::ACCEPT)
    .is_none_or(|value| value.to_str().is_ok_and(|v| v.contains("text/html")))
    && let Some(options) = use_context::<ResponseOptions>()
  {
    options.set_status(StatusCode::SEE_OTHER);
    options.insert_header(header::LOCATION, HeaderValue::from_static(path));
  }
}
