use crate::dto::RequestRowDto;
use leptos::prelude::*;
use leptos_struct_table::{CellValue, TableRow};

#[derive(Clone, TableRow)]
struct RequestTableRow {
  #[table(title = "请求标识")]
  request_id: String,
  #[table(title = "模型")]
  model: String,
  #[table(title = "项目 / Key")]
  identity: RequestIdentity,
  #[table(title = "Token")]
  tokens: TokenCounts,
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
      <thead><tr><th scope="col">"请求标识"</th><th scope="col">"模型"</th><th scope="col">"项目 / Key"</th><th scope="col">"Token"</th><th scope="col">"结果"</th><th scope="col">"用时"</th><th scope="col">"策略版本"</th></tr></thead>
      <tbody>{records.into_iter().enumerate().map(|(index, record)| {
        let row = RwSignal::new(RequestTableRow { request_id: record.request_id, model: record.model,
          identity: RequestIdentity { project: record.project_id, key: record.key_id },
          tokens: TokenCounts { input: record.input_tokens, output: record.output_tokens,
            cached: record.cached_input_tokens, reasoning: record.reasoning_tokens, reserved: record.reserved_tokens },
          status: status_label(&record.status).into(), elapsed: format!("{} ms", record.elapsed_ms), policy_revision: record.policy_revision });
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

#[derive(Clone)]
struct RequestIdentity {
  project: Option<String>,
  key: Option<String>,
}
impl CellValue for RequestIdentity {
  type RenderOptions = ();
  fn render_value(self, _: ()) -> impl IntoView {
    view! { <div><span>{format!("项目 {}", self.project.as_deref().unwrap_or("未知"))}</span><br/>
    <span class="muted">{format!("Key {}", self.key.as_deref().unwrap_or("未知"))}</span></div> }
  }
}
#[derive(Clone)]
struct TokenCounts {
  input: Option<u64>,
  output: Option<u64>,
  cached: Option<u64>,
  reasoning: Option<u64>,
  reserved: u64,
}
impl CellValue for TokenCounts {
  type RenderOptions = ();
  fn render_value(self, _: ()) -> impl IntoView {
    view! { <div><span>{format!("输入 {} / 输出 {}", token_count(self.input), token_count(self.output))}</span><br/>
    <span class="muted">{format!("缓存 {} / 推理 {}", token_count(self.cached), token_count(self.reasoning))}</span><br/>
    <span class="muted">{format!("初始预留 {}", self.reserved)}</span></div> }
  }
}
fn token_count(value: Option<u64>) -> String {
  value
    .map(|value| value.to_string())
    .unwrap_or_else(|| "未知".into())
}
