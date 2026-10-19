use crate::dto::DomainRowDto;
use leptos::prelude::*;
use leptos_struct_table::TableRow;

#[derive(Clone, TableRow)]
struct DomainTableRow {
  #[table(title = "任务类别", class = "model-name")]
  label: String,
  #[table(title = "推荐档位")]
  tier: String,
  #[table(title = "匹配词")]
  terms: String,
}
#[component]
pub fn DomainTable(domains: Vec<DomainRowDto>) -> impl IntoView {
  let columns = RwSignal::new(DomainTableRow::columns().to_vec());
  view! {
    <div class="table-wrap"><table aria-label="任务分类与模型档位">
      <thead><tr><th scope="col">"任务类别"</th><th scope="col">"推荐档位"</th><th scope="col">"匹配词"</th></tr></thead>
      <tbody>{domains.into_iter().enumerate().map(|(index, domain)| {
        let row = RwSignal::new(DomainTableRow { label: domain.label, tier: tier_label(&domain.tier).into(), terms: domain.terms.join(" · ") });
        view! { <tr>{DomainTableRow::render_row(row, index, columns)}</tr> }
      }).collect_view()}</tbody>
    </table></div>
  }
}
fn tier_label(tier: &str) -> &'static str {
  match tier {
    "fast" => "快速",
    "balanced" => "均衡",
    "advanced" => "高级",
    _ => "未知",
  }
}
