use crate::dto::ModelRowDto;
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_query_map};
use leptos_struct_table::TableRow;

const MODEL_PAGE_SIZE: usize = 25;
#[derive(Clone, TableRow)]
struct ModelTableRow {
  #[table(title = "模型标识", class = "model-name")]
  id: String,
  #[table(title = "目录归属", class = "model-kind")]
  owned_by: String,
  #[table(title = "状态")]
  state: String,
}
#[component]
pub fn ModelTable(models: Vec<ModelRowDto>) -> impl IntoView {
  let count = models.len();
  let page_count = count.div_ceil(MODEL_PAGE_SIZE).max(1);
  let models = StoredValue::new(models);
  let query = use_query_map();
  let page = Memo::new(move |_| {
    query
      .read()
      .get("page")
      .and_then(|value| value.parse::<usize>().ok())
      .unwrap_or(0)
      .min(page_count - 1)
  });
  let columns = RwSignal::new(ModelTableRow::columns().to_vec());
  view! {
    <div class="table-wrap"><table aria-label="已配置模型目录">
      <thead><tr><th scope="col">"模型标识"</th><th scope="col">"目录归属"</th><th scope="col">"状态"</th></tr></thead>
      <tbody>{move || models.with_value(|models| models.iter().skip(page.get() * MODEL_PAGE_SIZE).take(MODEL_PAGE_SIZE).enumerate().map(|(index, model)| {
        let row = RwSignal::new(ModelTableRow { id: model.id.clone(), owned_by: model.owned_by.clone(), state: "已注册".into() });
        view! { <tr>{ModelTableRow::render_row(row, index, columns)}</tr> }
      }).collect_view())}</tbody>
    </table></div>
    {(count > MODEL_PAGE_SIZE).then(|| view! {
      <div class="pagination"><span class="muted">{move || format!("第 {} / {page_count} 页，共 {count} 个模型", page.get() + 1)}</span><div class="pagination-links">
        <Show when={move || page.get() > 0}><A href=move || format!("/ui/models?page={}", page.get().saturating_sub(1))>"上一页"</A></Show>
        <Show when={move || page.get() + 1 < page_count}><A href=move || format!("/ui/models?page={}", page.get() + 1)>"下一页"</A></Show>
      </div></div>
    })}
  }
}
