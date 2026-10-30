use crate::{
  api,
  components::{Notice, SubmitButton},
};
use leptos::{form::ActionForm, prelude::*};
use leptos_router::{
  components::A,
  hooks::{use_location, use_query_map},
};

#[component]
pub fn Brand(#[prop(default = "/ui/models")] href: &'static str) -> impl IntoView {
  view! {
    <a href=href class="brand"><span class="brand-mark" aria-hidden="true">"L"</span>
      <div>"Lyonbrown"<span>"AI Gateway"</span></div>
    </a>
  }
}
#[component]
pub fn Navigation() -> impl IntoView {
  view! {
    <nav aria-label="管理导航">
      <A href="/ui/models" exact=true>"模型"</A>
      <A href="/ui/policy" exact=true>"路由策略"</A>
      <A href="/ui/requests" exact=true>"请求记录"</A>
      <A href="/ui/keys" exact=true>"API Keys"</A>
      <a href="/swagger-ui/" rel="external">"API 文档"</a>
    </nav>
  }
}
#[component]
pub fn AppShell(csrf: String, children: Children) -> impl IntoView {
  let logout = ServerAction::<api::Logout>::new();
  let query = use_query_map();
  let location = use_location();
  let submit = move |event: leptos::ev::SubmitEvent| {
    if logout.pending().get_untracked() {
      event.prevent_default();
    }
  };
  let error = move || {
    if logout.value().get().is_some_and(|result| result.is_err()) {
      return Some("退出登录失败，请刷新页面后重试。".to_owned());
    }
    if location.pathname.get() != "/ui/models" {
      return None;
    }
    match query.read().get("error").as_deref() {
      Some("csrf") => Some("表单已失效，请刷新页面后重试。".to_owned()),
      Some("unavailable") => Some("服务暂时不可用，请稍后重试。".to_owned()),
      _ => None,
    }
  };
  view! {
    <a class="skip-link" href="#main">"跳转到正文"</a>
    <div class="app"><aside class="sidebar"><Brand/><Navigation/>
      <div class="sidebar-foot"><p>"超级管理员"</p>
        <ActionForm action=logout on:submit:capture=submit><input type="hidden" name="csrf" value=csrf/>
          <div class="form-actions"><SubmitButton label="退出登录" pending=logout.pending() secondary=true/></div>
        </ActionForm>
      </div>
    </aside><main id="main">{move || error().map(|message| view! { <Notice message error=true/> })}{children()}</main></div>
  }
}
