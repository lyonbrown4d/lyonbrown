use std::{collections::BTreeMap, fs};

use agent_config::{
  CredentialSource, CredentialState, ProviderStatus, load_with_environment, provider_definition,
};
use anyhow::{Context, Result};

fn provider<'a>(inventory: &'a [ProviderStatus], id: &str) -> Result<&'a ProviderStatus> {
  inventory
    .iter()
    .find(|entry| entry.definition.id == id)
    .context("provider not in inventory")
}

#[test]
fn absent_credentials_do_not_claim_reachability_or_auto_select_a_provider() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let settings = load_with_environment(temp.path(), None, &BTreeMap::new())?;
  let inventory = settings.provider_inventory();
  let openai = provider(&inventory, "openai")?;
  assert!(openai.selected);
  assert_eq!(openai.credential.state, CredentialState::Missing);
  assert_eq!(openai.configured_models.len(), 1);
  assert_eq!(openai.configured_models[0].id, "gpt-4.1");
  let anthropic = provider(&inventory, "anthropic")?;
  assert!(!anthropic.selected);
  assert!(anthropic.configured_models.is_empty());
  assert_eq!(anthropic.credential.state, CredentialState::Missing);
  assert_eq!(
    provider(&inventory, "local")?.credential.state,
    CredentialState::NotRequired
  );
  Ok(())
}

#[test]
fn effective_sources_follow_existing_precedence_without_exposing_credentials() -> Result<()> {
  let temp = tempfile::tempdir()?;
  fs::write(
    temp.path().join(".env"),
    "OPENAI_API_KEY=private-dotenv-openai\nANTHROPIC_API_KEY=private-dotenv-anthropic\n",
  )?;
  let environment = BTreeMap::from([
    ("OPENAI_API_KEY".into(), "private-environment-openai".into()),
    (
      "UNRELATED_VENDOR_API_KEY".into(),
      "private-unrelated".into(),
    ),
  ]);
  let settings = load_with_environment(temp.path(), None, &environment)?;
  let inventory = settings.provider_inventory();
  let openai = provider(&inventory, "openai")?;
  assert_eq!(openai.credential.state, CredentialState::Detected);
  assert_eq!(
    openai.credential.source,
    Some(CredentialSource::DaemonEnvironment)
  );
  assert_eq!(
    openai.credential.environment_name.as_deref(),
    Some("OPENAI_API_KEY")
  );
  assert_eq!(
    provider(&inventory, "anthropic")?.credential.source,
    Some(CredentialSource::Dotenv)
  );
  assert_eq!(
    settings.credential("openai").as_deref(),
    Some("private-environment-openai")
  );
  let serialized = serde_json::to_string(&inventory)?;
  assert!(!serialized.contains("private-"));
  assert!(!serialized.contains("UNRELATED_VENDOR"));
  assert!(!format!("{settings:?}").contains("private-"));
  assert_eq!(settings.mcp_environment_value("OPENAI_API_KEY"), None);
  assert_eq!(settings.command_environment_value("OPENAI_API_KEY"), None);
  Ok(())
}

#[test]
fn empty_environment_masks_dotenv_and_invalid_credentials_are_not_usable() -> Result<()> {
  let temp = tempfile::tempdir()?;
  fs::write(temp.path().join(".env"), "OPENAI_API_KEY=private-dotenv\n")?;
  let environment = BTreeMap::from([
    ("OPENAI_API_KEY".into(), "   ".into()),
    ("ANTHROPIC_API_KEY".into(), "private-invalid\0value".into()),
  ]);
  let settings = load_with_environment(temp.path(), None, &environment)?;
  let inventory = settings.provider_inventory();
  assert_eq!(
    provider(&inventory, "openai")?.credential.state,
    CredentialState::Missing
  );
  assert_eq!(
    provider(&inventory, "anthropic")?.credential.state,
    CredentialState::Invalid
  );
  assert_eq!(settings.credential("openai"), None);
  assert_eq!(settings.credential("anthropic"), None);
  assert!(!serde_json::to_string(&inventory)?.contains("private-"));
  Ok(())
}

#[test]
fn detected_unsupported_provider_is_not_enabled_and_cli_aliases_still_select() -> Result<()> {
  let temp = tempfile::tempdir()?;
  let environment = BTreeMap::from([
    ("GEMINI_API_KEY".into(), "private-google".into()),
    ("OPENROUTER_API_KEY".into(), "private-router".into()),
  ]);
  let mut settings = load_with_environment(temp.path(), None, &environment)?;
  let inventory = settings.provider_inventory();
  for id in ["gemini", "openrouter"] {
    let entry = provider(&inventory, id)?;
    assert!(!entry.selected);
    assert!(!entry.definition.runtime_supported);
    assert_eq!(entry.credential.state, CredentialState::Detected);
    assert_eq!(settings.credential(id), None);
  }
  // Inventory reflects explicit post-load CLI overrides; credentials do not
  // select a model and cannot turn recognition into runtime integration.
  settings.provider = "openai-completions".into();
  settings.model = "explicit-model".into();
  let inventory = settings.provider_inventory();
  assert_eq!(
    provider(&inventory, "openai")?.configured_models[0].id,
    "explicit-model"
  );
  assert_eq!(
    provider_definition("openai-responses").map(|entry| entry.id),
    Some("openai")
  );
  settings.provider = "unsupported-provider".into();
  assert!(provider_definition(&settings.provider).is_none());
  assert!(
    settings
      .provider_inventory()
      .iter()
      .all(|entry| !entry.selected)
  );
  Ok(())
}
