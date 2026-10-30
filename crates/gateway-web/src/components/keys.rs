use crate::key_form::KeyDraft;
use crate::{
  components::{
    AppShell, EmptyState, FormField, LoadError, LoadingPage, Notice, PageHeader, SubmitButton,
  },
  dto::{KeyRowDto, KeysView},
  key_api,
};
use garde::Validate;
use leptos::{form::ActionForm, prelude::*};
use leptos_meta::Title;
use leptos_router::hooks::use_query_map;

#[component]
pub fn KeysPage() -> impl IntoView {
  let create = ServerAction::<key_api::CreateKey>::new();
  let revoke = ServerAction::<key_api::RevokeKey>::new();
  let refresh = RwSignal::new((0_usize, 0_usize));
  Effect::new(move || {
    if create.value().get().is_some_and(|result| result.is_ok())
      || revoke.value().get().is_some_and(|result| result.is_ok())
    {
      refresh.set((
        create.version().get_untracked(),
        revoke.version().get_untracked(),
      ));
    }
  });
  let data = Resource::new(move || refresh.get(), |_| key_api::get_keys());
  view! {
    <Title text="API Keys · Lyonbrown Gateway"/>
    <Suspense fallback=LoadingPage>
      {move || data.get().map(|result| match result {
        Ok(data) => view! { <KeyManager data create revoke/> }.into_any(),
        Err(_) => view! { <LoadError/> }.into_any(),
      })}
    </Suspense>
  }
}
#[component]
fn KeyManager(
  data: KeysView,
  create: ServerAction<key_api::CreateKey>,
  revoke: ServerAction<key_api::RevokeKey>,
) -> impl IntoView {
  let query = use_query_map();
  let failure = move || {
    create.value().get().is_some_and(|result| result.is_err())
      || revoke.value().get().is_some_and(|result| result.is_err())
      || query.read().get("error").is_some()
  };
  view! {
    <AppShell csrf=data.csrf.clone()>
      <PageHeader title="API Keys" description="为项目创建受限访问凭据，查看 Token 预算并撤销访问。"/>
      <Show when=failure><Notice message="操作未完成。请检查输入，或刷新页面后重试。".to_owned() error=true/></Show>
      <CreateKeyForm csrf=data.csrf.clone() action=create/>
      <section class="section" aria-labelledby="keys-list-title">
        <div class="section-heading"><h2 id="keys-list-title">"已签发凭据"</h2><p class="muted">"已用预算包含未结算的保守预留；未知结果保留预留。"</p></div>
        {(data.keys.is_empty()).then(|| view! { <EmptyState title="暂无 API Key" description="创建后将显示项目、模型范围和预算。"/> })}
        <div class="table-wrap"><table aria-label="API Key 元数据">
          <thead><tr><th scope="col">"名称 / 项目"</th><th scope="col">"允许模型"</th><th scope="col">"输出上限"</th><th scope="col">"已用 / 总预算"</th><th scope="col">"状态"</th><th scope="col">"操作"</th></tr></thead>
          <tbody>{data.keys.into_iter().map(|key| view! { <KeyRow key csrf=data.csrf.clone() revoke/> }).collect_view()}</tbody>
        </table></div>
      </section>
    </AppShell>
  }
}
#[component]
fn CreateKeyForm(csrf: String, action: ServerAction<key_api::CreateKey>) -> impl IntoView {
  let hydrated = RwSignal::new(false);
  let label = RwSignal::new(String::new());
  let project = RwSignal::new("default".to_owned());
  let models = RwSignal::new(String::new());
  let output = RwSignal::new(String::new());
  let budget = RwSignal::new(String::new());
  let ttl = RwSignal::new("2592000".to_owned());
  let invalid = Memo::new(move |_| {
    let (label, project, models, output, budget, ttl) = (
      label.get(),
      project.get(),
      models.get(),
      output.get(),
      budget.get(),
      ttl.get(),
    );
    KeyDraft {
      label: &label,
      project_id: &project,
      allowed_models: &models,
      max_output_tokens: &output,
      token_budget: &budget,
      ttl_seconds: ttl.parse().unwrap_or(0),
    }
    .validate()
    .is_err()
  });
  Effect::new(move || hydrated.set(true));
  let submit = move |event: leptos::ev::SubmitEvent| {
    if action.pending().get_untracked() || !hydrated.get_untracked() || invalid.get_untracked() {
      event.prevent_default();
    } else {
      action.value().set(None);
    }
  };
  view! {
    <section class="section" aria-labelledby="key-create-title">
      <div class="section-heading"><h2 id="key-create-title">"创建 API Key"</h2><p class="muted">"密钥仅在创建成功后显示一次，请立即保存。"</p></div>
      {move || action.value().get().and_then(Result::ok).map(|token| view! {
        <div class="form-body" role="status"><p>"新密钥（关闭后无法再次查看）"</p><code class="one-time-token">{token.display_once().to_owned()}</code>
          <p><button type="button" class="button-secondary" on:click=move |_| action.value().set(None)>"已保存，关闭密钥"</button></p>
        </div>
      })}
      <ActionForm action attr:class="form-body" on:submit:capture=submit>
        <input type="hidden" name="csrf" value=csrf/>
        <div class="form-grid">
          <FormField id="key-label" label="名称"><input id="key-label" name="label" prop:value=move || label.get() on:input=move |event| label.set(event_target_value(&event)) required maxlength="128" autocomplete="off"/></FormField>
          <FormField id="key-project" label="项目标识"><input id="key-project" name="project_id" prop:value=move || project.get() on:input=move |event| project.set(event_target_value(&event)) required maxlength="128" value="default" autocomplete="off"/></FormField>
          <FormField id="key-models" label="允许模型" hint="填写模型目录标识，以英文逗号分隔；留空允许全部模型。"><input id="key-models" name="allowed_models" prop:value=move || models.get() on:input=move |event| models.set(event_target_value(&event)) maxlength="129000" autocomplete="off"/></FormField>
          <FormField id="key-output" label="单次输出 Token 上限" hint="留空不额外限制输出。"><input id="key-output" name="max_output_tokens" prop:value=move || output.get() on:input=move |event| output.set(event_target_value(&event)) type="number" min="0" max="9223372036854775807" step="1"/></FormField>
          <FormField id="key-budget" label="累计 Token 预算" hint="输入与输出合计，不按周期重置；留空不设置预算。"><input id="key-budget" name="token_budget" prop:value=move || budget.get() on:input=move |event| budget.set(event_target_value(&event)) type="number" min="0" max="9223372036854775807" step="1"/></FormField>
          <FormField id="key-ttl" label="有效期（秒）" hint="最多 31536000 秒（365 天）。"><input id="key-ttl" name="ttl_seconds" prop:value=move || ttl.get() on:input=move |event| ttl.set(event_target_value(&event)) type="number" required min="1" max="31536000" step="1" value="2592000"/></FormField>
        </div>
        <Show when=move || !label.get().is_empty() && invalid.get()><Notice message="请检查项目、模型列表、Token 范围和有效期。".to_owned() error=true/></Show>
        <div class="form-actions"><SubmitButton label="创建并显示密钥" pending=action.pending() disabled=Signal::derive(move || !hydrated.get() || invalid.get())/>
          <Show when=move || !hydrated.get()><p class="hint">"交互组件加载完成后可创建密钥。"</p></Show>
        </div>
      </ActionForm>
    </section>
  }
}
#[component]
fn KeyRow(key: KeyRowDto, csrf: String, revoke: ServerAction<key_api::RevokeKey>) -> impl IntoView {
  let submit = move |event: leptos::ev::SubmitEvent| {
    if revoke.pending().get_untracked() {
      event.prevent_default();
    }
  };
  view! {
    <tr><td><strong>{key.label}</strong><p class="muted">{key.project_id}</p></td>
      <td>{if key.allowed_models.is_empty() { "全部模型".into() } else { key.allowed_models.join(", ") }}</td>
      <td>{key.max_output_tokens.map(|value| value.to_string()).unwrap_or_else(|| "未设置".into())}</td>
      <td>{format!("{} / {}", key.tokens_charged, key.token_budget.map(|value| value.to_string()).unwrap_or_else(|| "未设置".into()))}</td>
      <td>{if key.revoked { "已撤销" } else { "已签发" }}</td>
      <td><ActionForm action=revoke on:submit:capture=submit><input type="hidden" name="csrf" value=csrf/><input type="hidden" name="id" value=key.id/>
        <SubmitButton label="撤销" pending=revoke.pending() disabled=key.revoked secondary=true/>
      </ActionForm></td>
    </tr>
  }
}
