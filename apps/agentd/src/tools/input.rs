use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Read {
  pub path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Write {
  pub path: String,
  pub content: String,
  pub expected_revision: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Discover {
  pub path: String,
  #[serde(default = "depth")]
  pub max_depth: usize,
  #[serde(default = "results")]
  pub max_results: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
  pub path: String,
  pub query: String,
  #[serde(default = "depth")]
  pub max_depth: usize,
  #[serde(default = "results")]
  pub max_results: usize,
}
fn depth() -> usize {
  4
}
fn results() -> usize {
  200
}
