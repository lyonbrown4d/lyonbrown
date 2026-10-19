use crate::{App, LeptosOptions};
use axum::http::{HeaderValue, header};
use leptos::prelude::*;
use leptos_axum::ResponseOptions;
use leptos_meta::MetaTags;
pub(crate) fn shell(options: LeptosOptions) -> impl IntoView {
  view! {
    <!DOCTYPE html>
    <html lang="zh-CN"><head><meta charset="utf-8"/><meta name="viewport" content="width=device-width, initial-scale=1"/>
      <HydrationScripts options/><MetaTags/>
    </head><body><App/></body></html>
  }
}
pub(crate) fn security_context() {
  if use_nonce().is_none() {
    leptos::nonce::provide_nonce();
  }
  if let (Some(nonce), Some(options)) = (use_nonce(), use_context::<ResponseOptions>()) {
    let csp = format!(
      "default-src 'none'; script-src 'self' 'nonce-{nonce}' 'wasm-unsafe-eval'; connect-src 'self'; style-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"
    );
    if let Ok(value) = HeaderValue::from_str(&csp) {
      options.insert_header(header::CONTENT_SECURITY_POLICY, value);
    }
  }
}
