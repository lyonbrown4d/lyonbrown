use leptos::prelude::*;

#[component]
pub fn PageHeader(
  title: &'static str,
  description: &'static str,
  #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
  view! {
    <div class="page-header">
      <div><h1>{title}</h1><p class="description">{description}</p></div>
      {children.map(|children| children())}
    </div>
  }
}
#[component]
pub fn Notice(message: String, #[prop(optional)] error: bool) -> impl IntoView {
  view! { <p class="notice" class:error=error role=if error { "alert" } else { "status" }>{message}</p> }
}
#[component]
pub fn FormField(
  id: &'static str,
  label: &'static str,
  children: Children,
  #[prop(optional)] hint: Option<&'static str>,
) -> impl IntoView {
  view! {
    <div class="field"><label for=id>{label}</label>
      {hint.map(|hint| view! { <span class="hint">{hint}</span> })}
      {children()}
    </div>
  }
}
#[component]
pub fn SubmitButton(
  label: &'static str,
  #[prop(into)] pending: Signal<bool>,
  #[prop(optional, into)] disabled: Signal<bool>,
  #[prop(optional)] secondary: bool,
) -> impl IntoView {
  view! {
    <button type="submit" class:button-secondary=secondary disabled=move || pending.get() || disabled.get() aria-busy=move || pending.get().to_string()>
      {move || if pending.get() { "正在提交…" } else { label }}
    </button>
  }
}
#[component]
pub fn LoadingPage() -> impl IntoView {
  view! { <main aria-busy="true"><p class="notice" role="status">"正在读取管理信息…"</p></main> }
}
#[component]
pub fn LoadError() -> impl IntoView {
  view! {
    <main><PageHeader title="暂时无法读取页面" description="请确认管理会话仍然有效，然后重新打开页面。"/>
      <a class="button button-secondary" href="/ui/login">"重新登录"</a>
    </main>
  }
}
#[component]
pub fn EmptyState(title: &'static str, description: &'static str) -> impl IntoView {
  view! { <div class="empty"><h2>{title}</h2><p class="muted">{description}</p></div> }
}
