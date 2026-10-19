use axum::{
  Json,
  http::StatusCode,
  response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct ErrorEnvelope {
  pub error: ErrorDetail,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct ErrorDetail {
  pub message: &'static str,
  pub r#type: &'static str,
  pub code: &'static str,
}
#[derive(Debug)]
pub struct GatewayError(pub StatusCode, pub &'static str, pub &'static str);
impl GatewayError {
  pub(crate) fn invalid(message: &'static str) -> Self {
    Self(StatusCode::BAD_REQUEST, "invalid_request", message)
  }
  pub(crate) fn unavailable() -> Self {
    Self(
      StatusCode::SERVICE_UNAVAILABLE,
      "audit_unavailable",
      "Request audit is unavailable",
    )
  }
  pub(crate) fn timeout() -> Self {
    Self(
      StatusCode::GATEWAY_TIMEOUT,
      "upstream_timeout",
      "Upstream deadline exceeded",
    )
  }
  pub(crate) fn routing_timeout() -> Self {
    Self(
      StatusCode::GATEWAY_TIMEOUT,
      "routing_timeout",
      "Routing deadline exceeded",
    )
  }
  pub(crate) fn upstream() -> Self {
    Self(
      StatusCode::BAD_GATEWAY,
      "upstream_error",
      "Upstream request failed",
    )
  }
}
impl IntoResponse for GatewayError {
  fn into_response(self) -> Response {
    (
      self.0,
      Json(ErrorEnvelope {
        error: ErrorDetail {
          message: self.2,
          r#type: "gateway_error",
          code: self.1,
        },
      }),
    )
      .into_response()
  }
}
