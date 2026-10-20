use utoipa::{
  Modify, OpenApi,
  openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct ModelList {
  pub object: String,
  pub data: Vec<crate::PublicModel>,
}
#[derive(OpenApi)]
#[openapi(info(title="Lyonbrown AI Gateway", version="0.1.0", description="Authenticated text and function-tool Chat Completions gateway. Streaming bytes, tool deltas and usage are preserved. Unsupported media content is rejected. Readiness validates local configuration; it does not probe upstream health."), components(schemas(crate::ChatRequestSchema, crate::RoutingConstraints, crate::ErrorEnvelope, crate::ErrorDetail, crate::PublicModel, ModelList)), modifiers(&Bearer))]
struct BaseDoc;

pub struct ApiDoc;
impl OpenApi for ApiDoc {
  fn openapi() -> utoipa::openapi::OpenApi {
    let (_, schema) = utoipa_axum::router::OpenApiRouter::with_openapi(BaseDoc::openapi())
      .merge(crate::http::protected_routes())
      .merge(crate::http::public_routes())
      .split_for_parts();
    schema
  }
}
struct Bearer;
impl Modify for Bearer {
  fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
    if let Some(components) = &mut openapi.components {
      components.add_security_scheme(
        "bearer",
        SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
      );
    }
  }
}
