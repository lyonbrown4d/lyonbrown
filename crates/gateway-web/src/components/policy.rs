use crate::{
  api,
  components::{
    AppShell, FormField, LoadError, LoadingPage, Notice, PageHeader, SubmitButton,
    tables::DomainTable,
  },
  dto::PolicyView,
};
use gateway_validation::{PolicyDraft, PolicyDraftErrors};
use leptos::{form::ActionForm, prelude::*};
use leptos_meta::Title;
use leptos_router::hooks::use_query_map;

#[component]
pub fn PolicyPage() -> impl IntoView {
  let save = ServerAction::<api::SavePolicy>::new();
  let refresh = RwSignal::new(0_usize);
  Effect::new(move || {
    if matches!(save.value().get(), Some(Ok(()))) {
      refresh.set(save.version().get_untracked());
    }
  });
  let data = Resource::new(move || refresh.get(), |_| api::get_policy());
  view! {
    <Title text="路由策略 · Lyonbrown Gateway"/>
    <Suspense fallback=LoadingPage>
      {move || data.get().map(|result| match result {
        Ok(data) => view! { <PolicyEditor data save/> }.into_any(),
        Err(_) => view! { <LoadError/> }.into_any(),
      })}
    </Suspense>
  }
}
#[component]
fn PolicyEditor(data: PolicyView, save: ServerAction<api::SavePolicy>) -> impl IntoView {
  let original = StoredValue::new(data.document.clone());
  let expected = StoredValue::new(data.policy_revision.clone());
  let document = RwSignal::new(data.document.clone());
  let revision = RwSignal::new(String::new());
  let hydrated = RwSignal::new(false);
  Effect::new(move || hydrated.set(true));
  let dirty = Memo::new(move |_| {
    document.with(|value| original.with_value(|original| value != original))
      || revision.with(|value| !value.is_empty())
  });
  let validation = Memo::new(move |_| {
    document.with(|document| {
      revision.with(|revision| {
        expected.with_value(|expected| {
          PolicyDraft {
            expected_revision: expected,
            revision,
            document,
          }
          .validate()
          .err()
          .unwrap_or(PolicyDraftErrors {
            revision: None,
            document: None,
          })
        })
      })
    })
  });
  let disabled =
    Signal::derive(move || hydrated.get() && (!dirty.get() || validation.get().invalid()));
  let submit = move |event: leptos::ev::SubmitEvent| {
    if save.pending().get_untracked() || validation.get_untracked().invalid() {
      event.prevent_default();
    }
  };
  let reset = move |event: leptos::ev::MouseEvent| {
    event.prevent_default();
    if !save.pending().get_untracked() {
      document.set(original.get_value());
      revision.set(String::new());
      save.value().set(None);
    }
  };
  let query = use_query_map();
  let server_error = move || {
    save
      .value()
      .get()
      .and_then(Result::err)
      .map(|error| {
        if error.to_string().contains("变化") || error.to_string().contains("conflict") {
          "策略已发生变化。请读取最新策略后重新编辑。".to_owned()
        } else {
          "策略未保存。请检查内容，或重新读取策略后重试。".to_owned()
        }
      })
      .or_else(|| match query.read().get("error").as_deref() {
        Some("validation") => Some("策略未保存。请检查版本名称和策略内容。".to_owned()),
        Some("conflict") => Some("策略已发生变化。请读取最新策略后重新编辑。".to_owned()),
        Some("csrf") => Some("表单已失效，请刷新页面后重试。".to_owned()),
        Some("unavailable") => Some("服务暂时不可用，请稍后重试。".to_owned()),
        _ => None,
      })
  };
  let saved = move || query.read().get("saved").as_deref() == Some("true");
  view! {
    <AppShell csrf=data.csrf.clone()>
      <PageHeader title="路由策略" description="设置任务分类和模型档位。变更通过校验后生效，并保留原有版本。"/>
      <Show when=saved><Notice message="策略已保存并应用。".to_owned()/></Show>
      {move || server_error().map(|message| view! { <Notice message error=true/><p><a href="/ui/policy">"读取最新策略"</a></p> })}
      <section class="section" aria-labelledby="policy-title">
        <div class="section-heading"><h2 id="policy-title">"当前策略"</h2><p class="muted">{data.policy_revision.clone()}</p></div>
        <DomainTable domains=data.domains/>
        <ActionForm action=save attr:class="form-body" on:submit:capture=submit>
          <input type="hidden" name="csrf" value=data.csrf/><input type="hidden" name="expected_revision" value=data.policy_revision.clone()/>
          <div class="form-grid">
            <FormField id="current-revision" label="当前版本"><input id="current-revision" readonly value=data.policy_revision/></FormField>
            <FormField id="revision" label="新版本名称"><input id="revision" name="revision" required maxlength="128" placeholder="为本次变更指定唯一名称" prop:value=move || revision.get() on:input=move |event| revision.set(event_target_value(&event)) aria-invalid=move || (dirty.get() && validation.get().revision.is_some()).to_string()/></FormField>
          </div>
          <FormField id="document" label="策略内容" hint="JSON 格式。保留所需字段；版本字段以新版本名称为准。">
            <SafePolicyTextarea value=document initial=data.document invalid=Signal::derive(move || dirty.get() && validation.get().document.is_some())/>
          </FormField>
          <div id="policy-validation" aria-live="polite">
            {move || if dirty.get() { validation.get().document.or(validation.get().revision).map(|message| view! { <Notice message=message.to_owned() error=true/> }) } else { None }}
          </div>
          <div class="form-actions"><SubmitButton label="保存并应用" pending=save.pending() disabled/>
            <button type="reset" class="button-secondary" on:click=reset disabled=move || save.pending().get() || (hydrated.get() && !dirty.get())>"重置更改"</button>
            <p class="hint" role="status">{move || if dirty.get() { "有未保存的更改" } else { "尚未修改" }}</p>
          </div>
        </ActionForm>
      </section>
    </AppShell>
  }
}

/// Leptos serializes textarea children as RCDATA. Escape only that SSR serialization boundary;
/// client DOM text and the value property always receive the original document.
#[component]
fn SafePolicyTextarea(
  value: RwSignal<String>,
  initial: String,
  invalid: Signal<bool>,
) -> impl IntoView {
  #[cfg(feature = "ssr")]
  let text = html_escape::encode_text(&initial).into_owned();
  #[cfg(not(feature = "ssr"))]
  let text = initial;
  view! {
    <textarea id="document" name="document" required rows="18" spellcheck="false" aria-describedby="policy-validation" aria-invalid=move || invalid.get().to_string()
      prop:value=move || value.get() on:input=move |event| value.set(event_target_value(&event))>{text}</textarea>
  }
}
