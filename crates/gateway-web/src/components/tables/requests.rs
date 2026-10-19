use crate::dto::RequestRowDto;
use leptos::prelude::*;
use leptos_struct_table::TableRow;

#[derive(Clone, TableRow)]
struct RequestTableRow {
  #[table(title = "请求标识")]
  request_id: String,
  #[table(title = "模型")]
  model: String,
  #[table(title = "结果")]
  status: String,
  #[table(title = "用时")]
  elapsed: String,
  #[table(title = "策略版本")]
  policy_revision: String,
}

#[component]
pub fn RequestTable(records: Vec<RequestRowDto>) -> impl IntoView {
  let columns = RwSignal::new(RequestTableRow::columns().to_vec());
  view! {
    <div class="table-wrap"><table aria-label="请求执行记录">
      <thead><tr><th scope="col">"请求标识"</th><th scope="col">"模型"</th><th scope="col">"结果"</th><th scope="col">"用时"</th><th scope="col">"策略版本"</th></tr></thead>
      <tbody>{records.into_iter().enumerate().map(|(index, record)| {
        let row = RwSignal::new(RequestTableRow { request_id: record.request_id, model: record.model, status: status_label(&record.status).into(), elapsed: format!("{} ms", record.elapsed_ms), policy_revision: record.policy_revision });
        view! { <tr>{RequestTableRow::render_row(row, index, columns)}</tr> }
      }).collect_view()}</tbody>
    </table></div>
  }
}
fn status_label(status: &str) -> &'static str {
  match status {
    "pending" => "处理中",
    "succeeded" => "已完成",
    "failed" => "失败",
    "cancelled" => "已取消",
    "uncertain" => "结果未知",
    _ => "未知",
  }
}
