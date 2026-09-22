use agent_context::{CompileRequest, PromptCompiler, TokenEstimator};
use agent_core::{
    ContextItem, ContextItemKind, DataClassification, Locality, ModelCapabilities, ModelTier,
};
use agent_profile::AgentProfile;
use agent_skill::Skill;
use agent_workspace::{ProjectType, RepositoryInstruction, WorkspaceProfile};

fn capabilities(supports_tools: bool) -> ModelCapabilities {
    ModelCapabilities {
        provider: "local".into(),
        model: "worker".into(),
        tier: ModelTier::Worker,
        locality: Locality::Local,
        context_window: 8_192,
        max_output_tokens: 1_024,
        supports_tools,
        supports_streaming: true,
        supports_json: true,
        supports_thinking: false,
        cost_per_mtok_in: 0.0,
        cost_per_mtok_out: 0.0,
    }
}

fn workspace() -> WorkspaceProfile {
    WorkspaceProfile {
        root: ".".into(),
        project_type: ProjectType::Rust,
        languages: vec!["Rust".into()],
        frameworks: vec![],
        build_commands: vec!["cargo build".into()],
        test_commands: vec!["cargo test".into()],
        conventions: vec![],
        enabled_skills: vec![],
        repository_instructions: vec![RepositoryInstruction {
            path: "AGENTS.md".into(),
            content: "Repository rules".into(),
        }],
    }
}

fn skill() -> Skill {
    Skill::from_yaml("name: rust\ntriggers: [rust]\ninstructions: Use idiomatic Rust.\n").unwrap()
}

#[test]
fn estimator_is_deterministic_and_rounds_up() {
    let estimator = TokenEstimator::default();
    assert_eq!(estimator.estimate(""), 0);
    assert_eq!(estimator.estimate("a"), 1);
    assert_eq!(estimator.estimate("abcde"), 2);
}

#[test]
fn compiler_emits_sections_in_the_required_order() {
    let profile = AgentProfile::builtin_default();
    let capabilities = capabilities(false);
    let workspace = workspace();
    let skills = [skill()];
    let result = PromptCompiler::default()
        .compile(CompileRequest {
            profile: &profile,
            model: &capabilities,
            workspace: &workspace,
            skills: &skills,
            tool_guidance: Some("Use the text tool protocol."),
            context_items: &[],
            token_budget: 8_000,
        })
        .unwrap();

    let headings = [
        "## Agent Profile",
        "## Model Capabilities",
        "## Workspace Profile",
        "## Repository Instructions",
        "## Matched Skills",
        "## Tool Guidance",
    ];
    let positions: Vec<_> = headings
        .iter()
        .map(|heading| result.system_prompt.find(heading).unwrap())
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(result.system_prompt.contains("Use idiomatic Rust."));
}

#[test]
fn native_tool_models_do_not_receive_text_protocol_guidance() {
    let profile = AgentProfile::builtin_default();
    let capabilities = capabilities(true);
    let workspace = workspace();
    let result = PromptCompiler::default()
        .compile(CompileRequest {
            profile: &profile,
            model: &capabilities,
            workspace: &workspace,
            skills: &[],
            tool_guidance: Some("text protocol"),
            context_items: &[],
            token_budget: 8_000,
        })
        .unwrap();
    assert!(!result.system_prompt.contains("## Tool Guidance"));
    assert!(!result.system_prompt.contains("text protocol"));
}

#[test]
fn budget_drops_large_low_priority_content_but_keeps_recent_critical_context() {
    let profile = AgentProfile::builtin_default();
    let capabilities = capabilities(true);
    let workspace = workspace();
    let mut items = vec![ContextItem::new(
        ContextItemKind::Document,
        "x".repeat(200 * 1024),
        250,
    )];
    for message in ["old user", "user one", "user two", "user three"] {
        let mut item = ContextItem::new(ContextItemKind::Message, message, 200);
        item.source = "user".into();
        items.push(item);
    }
    let mut tool = ContextItem::new(ContextItemKind::ToolObservation, "latest tool", 200);
    tool.source = "tool://shell#1".into();
    items.push(tool);

    let result = PromptCompiler::default()
        .compile(CompileRequest {
            profile: &profile,
            model: &capabilities,
            workspace: &workspace,
            skills: &[],
            tool_guidance: None,
            context_items: &items,
            token_budget: 8_000,
        })
        .unwrap();

    let retained: Vec<_> = result
        .context_items
        .iter()
        .map(|item| item.content.as_str())
        .collect();
    assert!(!retained.iter().any(|content| content.len() == 200 * 1024));
    assert!(retained.contains(&"user one"));
    assert!(retained.contains(&"user two"));
    assert!(retained.contains(&"user three"));
    assert!(retained.contains(&"latest tool"));
    assert!(result.dropped >= 1);
    assert!(result.system_tokens + result.context_tokens <= 8_000);
}

#[test]
fn budget_reserves_space_for_protected_items_before_optional_items() {
    let optional = ContextItem::new(ContextItemKind::Document, "o".repeat(32), 0);
    let mut user = ContextItem::new(ContextItemKind::Message, "u".repeat(32), 250);
    user.source = "user".into();

    let result = agent_context::ContextBudget::default()
        .trim(&[optional, user], 8)
        .unwrap();

    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].source, "user");
    assert_eq!(result.tokens, 8);
}

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
