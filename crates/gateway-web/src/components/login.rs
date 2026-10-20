use crate::{
  api,
  components::{Brand, FormField, Notice, SubmitButton},
};
use gateway_validation::Credentials;
use leptos::{form::ActionForm, prelude::*};
use leptos_meta::Title;
use leptos_router::hooks::use_query_map;

#[component]
pub fn LoginPage() -> impl IntoView {
  let login = ServerAction::<api::Login>::new();
  let query = use_query_map();
  let username = NodeRef::<leptos::html::Input>::new();
  let password = NodeRef::<leptos::html::Input>::new();
  let invalid = RwSignal::new(false);
  let submit = move |event: leptos::ev::SubmitEvent| {
    if login.pending().get_untracked() {
      event.prevent_default();
      return;
    }
    // Read the submitted DOM values so password-manager autofill is validated too.
    let valid = match (username.get_untracked(), password.get_untracked()) {
      (Some(username), Some(password)) => Credentials {
        username: &username.value(),
        password: &password.value(),
      }
      .validate()
      .is_ok(),
      _ => false,
    };
    invalid.set(!valid);
    if !valid {
      event.prevent_default();
    }
  };
  let error = move || {
    login
      .value()
      .get()
      .and_then(Result::err)
      .map(|error| {
        if error.to_string().contains("过多") {
          "登录尝试过多，请稍后重试。".to_owned()
        } else {
          "用户名或密码无效".to_owned()
        }
      })
      .or_else(|| invalid.get().then(|| "用户名或密码无效".to_owned()))
      .or_else(|| match query.read().get("error").as_deref() {
        Some("credentials") => Some("用户名或密码无效".to_owned()),
        Some("rate_limit") => Some("登录尝试过多，请稍后重试。".to_owned()),
        Some("origin") => Some("请求来源无效，请从管理台重新登录。".to_owned()),
        Some("unavailable") => Some("服务暂时不可用，请稍后重试。".to_owned()),
        _ => None,
      })
  };
  view! {
    <Title text="登录 · Lyonbrown Gateway"/>
    <div class="login-wrap"><main class="login"><Brand href="/ui/login"/>
      <h1>"登录管理台"</h1><p class="description">"使用超级管理员账户登录，管理模型、路由策略与请求记录。"</p>
      {move || error().map(|message| view! { <Notice message error=true/> })}
      <ActionForm action=login on:submit:capture=submit>
        <FormField id="username" label="用户名"><input id="username" name="username" type="text" autocomplete="username" autocapitalize="none" spellcheck="false" required minlength="1" maxlength="64" pattern=r"[A-Za-z0-9_.@\-]+" placeholder="输入超级管理员用户名" node_ref=username on:input=move |_| invalid.set(false)/></FormField>
        <FormField id="password" label="密码"><input id="password" name="password" type="password" autocomplete="current-password" required maxlength="128" placeholder="输入超级管理员密码" node_ref=password on:input=move |_| invalid.set(false)/></FormField>
        <SubmitButton label="登录" pending=login.pending()/>
      </ActionForm>
      <span class="hint">"账户由部署管理员配置。请在受信任的设备上登录。"</span>
    </main></div>
  }
}
