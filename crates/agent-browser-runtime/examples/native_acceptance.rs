//! Dev-only real CEF process driver for synthetic acceptance profiles.
//! Never installed as a product; production clients use dispatch_entry instead.
fn main() -> anyhow::Result<std::process::ExitCode> {
  #[cfg(debug_assertions)]
  let entry = agent_browser_runtime::dispatch_acceptance_entry()?;
  #[cfg(not(debug_assertions))]
  let entry = agent_browser_runtime::dispatch_entry()?;
  entry.ok_or_else(|| anyhow::anyhow!("this dev fixture only accepts native runtime/helper entry"))
}
