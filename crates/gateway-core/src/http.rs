use crate::{Gateway, GatewayError, request::ValidatedRequest};
use axum::{
  Json, Router,
  body::{Body, to_bytes},
  extract::{Request, State},
  http::{HeaderValue, StatusCode, header},
  middleware::{self, Next},
  response::{IntoResponse, Response},
};
use tokio::time::{Instant, timeout};
use utoipa_axum::{router::OpenApiRouter, routes};

pub(crate) fn protected_routes() -> OpenApiRouter<Gateway> {
  OpenApiRouter::default()
    .routes(routes!(models))
    .routes(routes!(completions))
}

pub(crate) fn public_routes() -> OpenApiRouter<Gateway> {
  OpenApiRouter::default()
    .routes(routes!(live))
    .routes(routes!(ready))
}

pub(crate) fn router(gateway: Gateway) -> Router {
  let protected = protected_routes().route_layer(middleware::from_fn_with_state(
    gateway.clone(),
    authenticate,
  ));
  let (router, _) = protected.merge(public_routes()).split_for_parts();
  router
    .with_state(gateway.clone())
    .merge(Gateway::documentation_router())
    .layer(middleware::from_fn_with_state(gateway, request_id))
}
async fn request_id(State(gateway): State<Gateway>, mut request: Request, next: Next) -> Response {
  let started = Instant::now();
  let id = uuid::Uuid::now_v7().to_string();
  request.extensions_mut().insert(id.clone());
  let mut response = next.run(request).await;
  let elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
  gateway
    .inner
    .signals
    .response(response.status().is_success(), elapsed_ms);
  tracing::info!(request_id=%id, operation="gateway.http", status=response.status().as_u16(), elapsed_ms, "gateway response headers sent");
  if let Ok(value) = HeaderValue::from_str(&id) {
    response.headers_mut().insert("x-request-id", value);
  }
  response
}
async fn authenticate(State(gateway): State<Gateway>, request: Request, next: Next) -> Response {
  let supplied = request
    .headers()
    .get(header::AUTHORIZATION)
    .and_then(|h| h.to_str().ok())
    .and_then(|s| s.strip_prefix("Bearer "))
    .unwrap_or("");
  if supplied.len() > 4096
    || request
      .headers()
      .get_all(header::AUTHORIZATION)
      .iter()
      .count()
      != 1
  {
    return unauthorized();
  }
  let authorized = tokio::time::timeout(
    std::time::Duration::from_millis(gateway.inner.config.limits.first_byte_ms),
    gateway.inner.access.authorize(supplied),
  )
  .await;
  let authorized = match authorized {
    Ok(Ok(authorized)) => authorized,
    _ => {
      return GatewayError(
        StatusCode::SERVICE_UNAVAILABLE,
        "access_unavailable",
        "Access verification is unavailable",
      )
      .into_response();
    }
  };
  if !authorized {
    return unauthorized();
  }
  next.run(request).await
}
fn unauthorized() -> Response {
  let mut response = GatewayError(
    StatusCode::UNAUTHORIZED,
    "unauthorized",
    "Bearer authentication required",
  )
  .into_response();
  response
    .headers_mut()
    .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
  response
}
#[utoipa::path(get, path="/health/live", responses((status=200, description="Process is serving")))]
async fn live() -> Json<serde_json::Value> {
  Json(serde_json::json!({"status":"live"}))
}
#[utoipa::path(get, path="/health/ready", responses((status=200, description="Validated registry is ready"), (status=503, description="Gateway draining or unavailable")))]
async fn ready(State(gateway): State<Gateway>) -> Response {
  if gateway.is_ready() {
    Json(serde_json::json!({"status":"ready"})).into_response()
  } else {
    (
      StatusCode::SERVICE_UNAVAILABLE,
      Json(serde_json::json!({"status":"not_ready"})),
    )
      .into_response()
  }
}
#[utoipa::path(get, path="/v1/models", security(("bearer"=[])), responses((status=200, description="Public configured model IDs", body=crate::openapi::ModelList), (status=401, body=crate::ErrorEnvelope)))]
async fn models(State(gateway): State<Gateway>) -> Json<crate::openapi::ModelList> {
  Json(crate::openapi::ModelList {
    object: "list".into(),
    data: gateway.models(),
  })
}
#[utoipa::path(post, path="/v1/chat/completions", security(("bearer"=[])), request_body=crate::ChatRequestSchema, responses((status=200, description="OpenAI JSON response or byte-preserved text/event-stream", content((serde_json::Value = "application/json"), (String = "text/event-stream"))), (status=400, body=crate::ErrorEnvelope), (status=401, body=crate::ErrorEnvelope), (status=404, body=crate::ErrorEnvelope), (status=413, body=crate::ErrorEnvelope), (status=429, body=crate::ErrorEnvelope), (status=502, body=crate::ErrorEnvelope), (status=503, body=crate::ErrorEnvelope), (status=504, body=crate::ErrorEnvelope)))]
async fn completions(
  State(gateway): State<Gateway>,
  request: Request<Body>,
) -> Result<Response, GatewayError> {
  if !gateway.is_ready() {
    return Err(GatewayError(
      StatusCode::SERVICE_UNAVAILABLE,
      "not_ready",
      "Gateway is not ready",
    ));
  }
  let started = Instant::now();
  let permit = gateway
    .inner
    .permits
    .clone()
    .try_acquire_owned()
    .map_err(|_| {
      GatewayError(
        StatusCode::TOO_MANY_REQUESTS,
        "capacity_exceeded",
        "Gateway is at capacity",
      )
    })?;
  let id = request
    .extensions()
    .get::<String>()
    .cloned()
    .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
  if !request
    .headers()
    .get(header::CONTENT_TYPE)
    .and_then(|v| v.to_str().ok())
    .is_some_and(|v| v.split(';').next() == Some("application/json"))
  {
    return Err(GatewayError::invalid(
      "Content-Type must be application/json",
    ));
  }
  let limits = &gateway.inner.config.limits;
  let bytes = timeout(
    std::time::Duration::from_millis(limits.first_byte_ms),
    to_bytes(request.into_body(), limits.max_body_bytes),
  )
  .await
  .map_err(|_| GatewayError::timeout())?
  .map_err(|_| {
    GatewayError(
      StatusCode::PAYLOAD_TOO_LARGE,
      "body_limit",
      "Request body exceeds the limit",
    )
  })?;
  let body =
    serde_json::from_slice(&bytes).map_err(|_| GatewayError::invalid("Malformed JSON body"))?;
  let request = ValidatedRequest::parse(body, limits.default_output_tokens)?;
  let deadline = started + std::time::Duration::from_millis(limits.first_byte_ms);
  let (model, revision) = gateway.select(&id, &request, deadline).await?;
  tracing::info!(request_id=%id, model=%model.id, policy_revision=%revision, operation="gateway.completion", "gateway route selected");
  gateway
    .transfer(id, request, model, revision, permit, started)
    .await
}
