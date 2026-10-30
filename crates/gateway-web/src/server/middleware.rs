use super::{WebState, auth, errors::WebError};
use axum::{
  body::Body,
  extract::{Request, State},
  http::{HeaderValue, Method, StatusCode, header},
  middleware::Next,
  response::{IntoResponse, Redirect, Response},
};
use governor::clock::Clock;
use leptos::{prelude::ServerFnError, server_fn::error::FromServerFnError};

pub(crate) async fn guard(
  State(state): State<WebState>,
  mut request: Request,
  next: Next,
) -> Response {
  let path = request.uri().path().to_owned();
  let native_form = request
    .headers()
    .get(header::ACCEPT)
    .and_then(|v| v.to_str().ok())
    .is_some_and(|v| v.contains("text/html"))
    && request.method() == Method::POST;
  let mutation = request.method() == Method::POST
    && matches!(
      path.as_str(),
      "/ui/login" | "/ui/logout" | "/ui/policy" | "/ui/keys/create" | "/ui/keys/revoke"
    );
  if mutation && !auth::origin_valid(&state, request.headers()) {
    return headers(action_error(WebError::forbidden(), &path, native_form));
  }
  if path.starts_with("/ui/api/")
    && let Err(error) = auth::authenticated(&state, request.headers()).await
  {
    return headers(fn_error(error));
  }
  if request.method() == Method::GET
    && matches!(
      path.as_str(),
      "/ui/models" | "/ui/policy" | "/ui/requests" | "/ui/keys"
    )
  {
    if let Err(error) = auth::authenticated(&state, request.headers()).await {
      return headers(if error.status == StatusCode::UNAUTHORIZED {
        Redirect::to("/ui/login").into_response()
      } else {
        error.into_response()
      });
    }
    if path == "/ui/requests" && !valid_pagination(request.uri().query()) {
      return headers((StatusCode::BAD_REQUEST, "分页参数无效。").into_response());
    }
  }
  if mutation
    && path == "/ui/login"
    && let Err(rejected) = state.attempts.check()
  {
    let wait = rejected.wait_time_from(state.attempts.clock().now());
    let retry_after = wait
      .as_secs()
      .saturating_add(u64::from(wait.subsec_nanos() > 0))
      .max(1);
    let mut response = action_error(
      WebError {
        status: StatusCode::TOO_MANY_REQUESTS,
        message: "登录尝试过多，请稍后重试。",
      },
      &path,
      native_form,
    );
    response
      .headers_mut()
      .insert(header::RETRY_AFTER, HeaderValue::from(retry_after));
    return headers(response);
  }
  let server_fn = mutation || path.starts_with("/ui/api/");
  // The framework may otherwise copy Referer and serialized argument errors into redirects.
  // Our native redirects use fixed URLs and error codes; enhanced clients receive typed errors.
  if server_fn {
    request.headers_mut().remove(header::REFERER);
  }
  let mut response = next.run(request).await;
  // Framework deserialization errors must never reflect submitted fields, passwords, or DB errors.
  let error_redirect = server_fn
    && response
      .headers()
      .get(header::LOCATION)
      .and_then(|v| v.to_str().ok())
      .is_some_and(|v| v.contains("__err="));
  if server_fn
    && (response.status().is_client_error()
      || response.status().is_server_error()
      || error_redirect)
  {
    if error_redirect {
      *response.status_mut() = StatusCode::BAD_REQUEST;
    }
    if native_form && mutation {
      return headers(action_error(
        WebError {
          status: response.status(),
          message: "请求未完成。",
        },
        &path,
        true,
      ));
    }
    let message = safe_error(response.status(), &path);
    let error: ServerFnError = ServerFnError::new(message);
    *response.body_mut() = Body::from(error.ser());
    response.headers_mut().remove(header::LOCATION);
    response.headers_mut().remove(header::CONTENT_LENGTH);
    response.headers_mut().insert(
      header::CONTENT_TYPE,
      HeaderValue::from_static("application/json"),
    );
  }
  headers(response)
}
fn action_error(error: WebError, path: &str, native: bool) -> Response {
  if !native {
    return fn_error(error);
  }
  let destination = match (path, error.status) {
    ("/ui/login", StatusCode::TOO_MANY_REQUESTS) => "/ui/login?error=rate_limit",
    ("/ui/login", StatusCode::FORBIDDEN) => "/ui/login?error=origin",
    ("/ui/login", StatusCode::SERVICE_UNAVAILABLE | StatusCode::INTERNAL_SERVER_ERROR) => {
      "/ui/login?error=unavailable"
    }
    ("/ui/login", _) => "/ui/login?error=credentials",
    ("/ui/policy", StatusCode::CONFLICT) => "/ui/policy?error=conflict",
    ("/ui/policy", StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED) => "/ui/policy?error=csrf",
    (
      "/ui/policy",
      StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY | StatusCode::PAYLOAD_TOO_LARGE,
    ) => "/ui/policy?error=validation",
    ("/ui/policy", _) => "/ui/policy?error=unavailable",
    ("/ui/keys/create" | "/ui/keys/revoke", _) => "/ui/keys?error=failed",
    (_, StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED) => "/ui/models?error=csrf",
    _ => "/ui/models?error=unavailable",
  };
  Redirect::to(destination).into_response()
}

fn fn_error(error: WebError) -> Response {
  let encoded: ServerFnError = ServerFnError::new(error.message);
  let mut response = (error.status, Body::from(encoded.ser())).into_response();
  response.headers_mut().insert(
    header::CONTENT_TYPE,
    HeaderValue::from_static("application/json"),
  );
  response
}

fn safe_error(status: StatusCode, path: &str) -> &'static str {
  match status {
    StatusCode::UNAUTHORIZED if path == "/ui/login" => "用户名或密码无效",
    StatusCode::UNAUTHORIZED => "请先登录管理台。",
    StatusCode::FORBIDDEN => "请求来源无效或表单已失效，请刷新页面重试。",
    StatusCode::CONFLICT => "策略已发生变化。请刷新页面后重新编辑。",
    StatusCode::UNPROCESSABLE_ENTITY => "策略校验失败。请检查 JSON 字段范围，并使用新的版本名称。",
    StatusCode::BAD_REQUEST => "请求参数无效。",
    StatusCode::PAYLOAD_TOO_LARGE => "请求内容过大，请精简后重试。",
    StatusCode::TOO_MANY_REQUESTS => "登录尝试过多，请稍后重试。",
    _ => "服务暂时不可用，请稍后重试。",
  }
}
fn headers(mut response: Response) -> Response {
  for (name, value) in [
    (header::CACHE_CONTROL, "no-store"),
    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (header::REFERRER_POLICY, "same-origin"),
  ] {
    response
      .headers_mut()
      .insert(name, HeaderValue::from_static(value));
  }
  if !response
    .headers()
    .contains_key(header::CONTENT_SECURITY_POLICY)
  {
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; style-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"));
  }
  response
}
fn valid_pagination(query: Option<&str>) -> bool {
  query.is_none_or(|query| {
    url::form_urlencoded::parse(query.as_bytes()).all(|(key, value)| match key.as_ref() {
      "limit" => value
        .parse::<u32>()
        .is_ok_and(|limit| (1..=200).contains(&limit)),
      "offset" => value
        .parse::<u64>()
        .is_ok_and(|offset| offset <= i64::MAX as u64),
      _ => true,
    })
  })
}
