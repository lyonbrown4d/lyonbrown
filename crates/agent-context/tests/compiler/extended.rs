use super::*;

#[test]
fn secret_context_is_filtered_before_ordinary_selection() {
  let mut secret = ContextItem::new(ContextItemKind::Document, "api-key", 0);
  secret.classification = DataClassification::Secret;
  let profile = AgentProfile::builtin_default();
  let capabilities = capabilities(true);
  let workspace = workspace();

  let result = PromptCompiler::default()
    .compile(CompileRequest {
      profile: &profile,
      model: &capabilities,
      workspace: &workspace,
      skills: &[],
      tool_guidance: None,
      context_items: &[secret],
      token_budget: 8_000,
    })
    .unwrap();

  assert!(result.context_items.is_empty());
  assert_eq!(result.dropped, 1);
}

#[test]
fn protected_latest_tool_observation_cannot_disclose_a_secret() {
  let mut secret = ContextItem::new(ContextItemKind::ToolObservation, "password", 0);
  secret.source = "tool://secret#1".into();
  secret.classification = DataClassification::Secret;
  let profile = AgentProfile::builtin_default();
  let capabilities = capabilities(true);
  let workspace = workspace();

  let result = PromptCompiler::default()
    .compile(CompileRequest {
      profile: &profile,
      model: &capabilities,
      workspace: &workspace,
      skills: &[],
      tool_guidance: None,
      context_items: &[secret],
      token_budget: 8_000,
    })
    .unwrap();

  assert!(result.context_items.is_empty());
  assert_eq!(result.dropped, 1);
}

#[test]
fn compiler_errors_when_system_prompt_exceeds_budget() {
  let profile = AgentProfile::builtin_default();
  let capabilities = capabilities(true);
  let workspace = workspace();
  let error = PromptCompiler::default()
    .compile(CompileRequest {
      profile: &profile,
      model: &capabilities,
      workspace: &workspace,
      skills: &[],
      tool_guidance: None,
      context_items: &[],
      token_budget: 1,
    })
    .unwrap_err();

  assert!(error.to_string().contains("system prompt"));
}

#[test]
fn compiler_errors_when_protected_minimum_exceeds_remaining_budget() {
  let profile = AgentProfile::builtin_default();
  let capabilities = capabilities(true);
  let workspace = workspace();
  let compiler = PromptCompiler::default();
  let baseline = compiler
    .compile(CompileRequest {
      profile: &profile,
      model: &capabilities,
      workspace: &workspace,
      skills: &[],
      tool_guidance: None,
      context_items: &[],
      token_budget: 8_000,
    })
    .unwrap();
  let mut user = ContextItem::new(ContextItemKind::Message, "x".repeat(40), 0);
  user.source = "user".into();

  let error = compiler
    .compile(CompileRequest {
      profile: &profile,
      model: &capabilities,
      workspace: &workspace,
      skills: &[],
      tool_guidance: None,
      context_items: &[user],
      token_budget: baseline.system_tokens + 9,
    })
    .unwrap_err();

  assert!(error.to_string().contains("protected context"));
}

#[test]
fn profile_fields_all_affect_the_compiled_prompt() {
  let mut profile = AgentProfile::builtin_default();
  profile.communication_style = Some("concise".into());
  profile.reasoning_policy = Some("verify assumptions".into());
  profile.privacy_policy = Some("keep data local".into());
  profile.tool_preferences = vec!["prefer read-only tools".into()];
  profile.default_skills = vec!["rust".into()];
  let capabilities = capabilities(true);
  let workspace = workspace();

  let result = PromptCompiler::default()
    .compile(CompileRequest {
      profile: &profile,
      model: &capabilities,
      workspace: &workspace,
      skills: &[],
      tool_guidance: None,
      context_items: &[],
      token_budget: 8_000,
    })
    .unwrap();

  for expected in [
    "prefer existing libraries=true",
    "avoid reinventing wheels=true",
    "inspect before editing=true",
    "run tests after changes=true",
    "architecture first=true",
    "avoid unnecessary abstraction=true",
    "prefer small reviewable changes=true",
    "reasoning policy: verify assumptions",
    "privacy policy: keep data local",
    "tool preferences: prefer read-only tools",
    "default skills: rust",
    "prefer local=true",
    "remote escalation=true",
  ] {
    assert!(
      result.system_prompt.contains(expected),
      "missing {expected}"
    );
  }
}
