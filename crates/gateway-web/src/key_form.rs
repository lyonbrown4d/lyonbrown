use garde::Validate;

#[derive(Validate)]
pub(crate) struct KeyDraft<'a> {
  #[garde(length(bytes, min = 1, max = 128), custom(clean))]
  pub label: &'a str,
  #[garde(length(bytes, min = 1, max = 128), custom(clean))]
  pub project_id: &'a str,
  #[garde(length(bytes, max = 129_000), custom(models))]
  pub allowed_models: &'a str,
  #[garde(custom(optional_limit))]
  pub max_output_tokens: &'a str,
  #[garde(custom(optional_limit))]
  pub token_budget: &'a str,
  #[garde(range(min = 1, max = 31_536_000))]
  pub ttl_seconds: u64,
}
#[cfg(feature = "ssr")]
impl KeyDraft<'_> {
  pub fn models(&self) -> std::collections::BTreeSet<String> {
    self
      .allowed_models
      .split(',')
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .map(str::to_owned)
      .collect()
  }
}
pub(crate) fn limit(value: &str) -> Result<Option<u64>, ()> {
  if value.is_empty() {
    return Ok(None);
  }
  value
    .parse::<u64>()
    .ok()
    .filter(|value| *value <= i64::MAX as u64)
    .map(Some)
    .ok_or(())
}
fn optional_limit(value: &str, _: &()) -> garde::Result {
  limit(value)
    .map(|_| ())
    .map_err(|_| garde::Error::new("invalid token limit"))
}
fn clean(value: &str, _: &()) -> garde::Result {
  if value.trim().is_empty() || value.chars().any(char::is_control) {
    return Err(garde::Error::new("invalid identifier"));
  }
  Ok(())
}
fn models(value: &str, _: &()) -> garde::Result {
  if value.len() > 129_000 {
    return Err(garde::Error::new("too many models"));
  }
  if value.is_empty() {
    return Ok(());
  }
  let entries: Vec<_> = value.split(',').map(str::trim).collect();
  if entries.len() > 1000
    || entries
      .iter()
      .any(|model| model.is_empty() || model.len() > 128 || clean(model, &()).is_err())
  {
    return Err(garde::Error::new("invalid models"));
  }
  Ok(())
}
