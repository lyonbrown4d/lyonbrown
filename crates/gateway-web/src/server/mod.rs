pub(crate) mod actions;
mod auth;
pub(crate) mod data;
mod errors;
mod middleware;
pub(crate) mod policy;
mod render;
use crate::LeptosOptions;
use axum::{Router, extract::FromRef, http::header, middleware as axum_middleware, routing::get};
use decision_core::DecisionEngine;
use gateway_core::Gateway;
use gateway_storage::GatewayStorage;
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use leptos_axum::{LeptosRoutes, generate_route_list};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tower_http::{limit::RequestBodyLimitLayer, services::ServeDir};

#[derive(Clone)]
pub(crate) struct WebState {
  store: Arc<GatewayStorage>,
  gateway: Arc<Gateway>,
  engine: Arc<DecisionEngine>,
  origin: Option<String>,
  secure: bool,
  options: LeptosOptions,
  attempts: Arc<DefaultDirectRateLimiter>,
  policy_write: Arc<AsyncMutex<()>>,
}
impl FromRef<WebState> for LeptosOptions {
  fn from_ref(state: &WebState) -> Self {
    state.options.clone()
  }
}
pub fn router(
  store: Arc<GatewayStorage>,
  gateway: Arc<Gateway>,
  engine: Arc<DecisionEngine>,
  origin: String,
) -> Router {
  router_with_options(
    store,
    gateway,
    engine,
    origin,
    LeptosOptions::builder()
      .output_name("gateway_web")
      .site_pkg_dir("ui/pkg")
      .build(),
  )
}
/// Origin and site root are trusted operator configuration. Invalid origins disable mutations.
pub fn router_with_options(
  store: Arc<GatewayStorage>,
  gateway: Arc<Gateway>,
  engine: Arc<DecisionEngine>,
  origin: String,
  options: LeptosOptions,
) -> Router {
  let parsed = url::Url::parse(&origin).ok().filter(|u| {
    matches!(u.scheme(), "http" | "https")
      && u.host_str().is_some()
      && (u.scheme() == "https"
        || u.host_str().is_some_and(|host| {
          host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
        }))
      && u.username().is_empty()
      && u.password().is_none()
      && u.query().is_none()
      && u.fragment().is_none()
      && u.path() == "/"
  });
  let state = WebState {
    store,
    gateway,
    engine,
    secure: parsed.as_ref().is_some_and(|u| u.scheme() == "https"),
    origin: parsed.map(|u| u.origin().ascii_serialization()),
    options: options.clone(),
    attempts: Arc::new(RateLimiter::direct(Quota::per_minute(
      nonzero_ext::nonzero!(10u32),
    ))),
    policy_write: Arc::new(AsyncMutex::new(())),
  };
  let routes = generate_route_list(crate::App)
    .into_iter()
    .filter(|route| route.path() != "/ui")
    .collect();
  let packages = std::path::Path::new(options.site_root.as_ref()).join("ui/pkg");
  Router::new()
    .route(
      "/ui",
      get(|| async { axum::response::Redirect::to("/ui/models") }),
    )
    .route(
      "/ui/assets/app.css",
      get(|| async {
        (
          [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
          include_str!("../style.css"),
        )
      }),
    )
    .nest_service("/ui/pkg", ServeDir::new(packages))
    .leptos_routes_with_context(&state, routes, render::security_context, move || {
      render::shell(options.clone())
    })
    .layer(RequestBodyLimitLayer::new(2_097_152))
    .layer(axum_middleware::from_fn_with_state(
      state.clone(),
      middleware::guard,
    ))
    .with_state(state)
}
