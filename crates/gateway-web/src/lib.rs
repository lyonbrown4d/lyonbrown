//! Leptos administration shared by server rendering and browser hydration.
#![forbid(unsafe_code)]
#![recursion_limit = "256"]
pub mod api;
mod app;
mod components;
pub mod dto;
pub mod key_api;
mod key_form;
pub use app::App;
pub use leptos::config::LeptosOptions;
#[cfg(feature = "ssr")]
mod server;
#[cfg(feature = "ssr")]
pub use server::{router, router_with_options};

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
  leptos::mount::hydrate_body(App);
}
