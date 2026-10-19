use crate::{
  api,
  components::{AppShell, EmptyState, LoadError, LoadingPage, PageHeader, tables::ModelTable},
  dto::ModelsView,
};
use leptos::prelude::*;
use leptos_meta::Title;

#[component]
pub fn ModelsPage() -> impl IntoView {
  let data = Resource::new(|| (), |_| api::get_models());
  view! {
    <Title text="模型 · Lyonbrown Gateway"/>
    <Suspense fallback=LoadingPage>
      {move || data.get().map(|result| match result {
        Ok(data) => view! { <ModelCatalog data/> }.into_any(),
        Err(_) => view! { <LoadError/> }.into_any(),
      })}
    </Suspense>
  }
}
#[component]
fn ModelCatalog(data: ModelsView) -> impl IntoView {
  let count = data.models.len();
  view! {
    <AppShell csrf=data.csrf>
      <PageHeader title="模型" description="当前网关的模型目录。请求可指定模型，也可使用自动路由。">
        <span class="status">{if data.ready { "网关已就绪" } else { "网关未就绪" }}</span>
      </PageHeader>
      <section class="section" aria-labelledby="catalog-title">
        <div class="section-heading"><h2 id="catalog-title">"已配置模型"</h2><p class="muted">{format!("共 {count} 个")}</p></div>
        <ModelTable models=data.models/>
        {(count == 0).then(|| view! { <EmptyState title="还没有模型" description="由部署管理员添加模型配置并重新加载网关后，模型会显示在这里。"/> })}
      </section>
    </AppShell>
  }
}
