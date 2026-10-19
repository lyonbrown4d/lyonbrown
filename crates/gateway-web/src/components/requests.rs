use crate::{
  api,
  components::{AppShell, EmptyState, LoadError, LoadingPage, PageHeader, tables::RequestTable},
  dto::RequestsView,
};
use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::{components::A, hooks::use_query_map};

#[component]
pub fn RequestsPage() -> impl IntoView {
  let query = use_query_map();
  let data = Resource::new(
    move || {
      let query = query.read();
      (
        query
          .get("offset")
          .and_then(|v| v.parse::<u64>().ok())
          .unwrap_or(0),
        query
          .get("limit")
          .and_then(|v| v.parse::<u32>().ok())
          .unwrap_or(25),
      )
    },
    |(offset, limit)| api::get_requests(offset, limit),
  );
  view! {
    <Title text="请求记录 · Lyonbrown Gateway"/>
    <Suspense fallback=LoadingPage>
      {move || data.get().map(|result| match result {
        Ok(data) => view! { <RequestLog data/> }.into_any(),
        Err(_) => view! { <LoadError/> }.into_any(),
      })}
    </Suspense>
  }
}
#[component]
fn RequestLog(data: RequestsView) -> impl IntoView {
  let shown = data.records.len() as u64;
  let offset = data.offset;
  let limit = data.limit;
  let total = data.total;
  view! {
    <AppShell csrf=data.csrf>
      <PageHeader title="请求记录" description="检查请求使用的模型、策略版本及执行结果。此页面仅保存请求元数据。"/>
      <section class="section" aria-labelledby="requests-title">
        <div class="section-heading"><h2 id="requests-title">"最近请求"</h2><p class="muted">{format!("共 {total} 条")}</p></div>
        <RequestTable records=data.records/>
        {(shown == 0).then(|| view! { <EmptyState title="暂无请求记录" description="网关开始处理请求后，执行结果会显示在这里。"/> })}
        <div class="pagination"><span class="muted">{if shown == 0 { "0 条记录".into() } else { format!("第 {}–{} 条", offset.saturating_add(1), offset.saturating_add(shown)) }}</span><div class="pagination-links">
          {(offset > 0).then(|| view! { <A href=format!("/ui/requests?offset={}&limit={limit}", offset.saturating_sub(u64::from(limit)))>"上一页"</A> })}
          {(offset.saturating_add(shown) < total).then(|| view! { <A href=format!("/ui/requests?offset={}&limit={limit}", offset.saturating_add(u64::from(limit)))>"下一页"</A> })}
        </div></div>
      </section>
    </AppShell>
  }
}
