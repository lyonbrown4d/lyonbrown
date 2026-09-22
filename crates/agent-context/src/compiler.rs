use std::fmt::Write;

use agent_core::{ContextItem, ModelCapabilities};
use agent_profile::AgentProfile;
use agent_skill::Skill;
use agent_workspace::WorkspaceProfile;

use crate::{BudgetError, ContextBudget, TokenEstimator};
use thiserror::Error;

pub struct CompileRequest<'a> {
    pub profile: &'a AgentProfile,
    pub model: &'a ModelCapabilities,
    pub workspace: &'a WorkspaceProfile,
    pub skills: &'a [Skill],
    pub tool_guidance: Option<&'a str>,
    pub context_items: &'a [ContextItem],
    pub token_budget: u32,
}

#[derive(Debug, Clone)]
pub struct CompiledPrompt {
    pub system_prompt: String,
    pub context_items: Vec<ContextItem>,
    pub system_tokens: u32,
    pub context_tokens: u32,
    pub dropped: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CompileError {
    #[error("system prompt requires {required} tokens but budget is {budget}")]
    SystemPromptExceeds { required: u64, budget: u32 },
    #[error(transparent)]
    Budget(#[from] BudgetError),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PromptCompiler {
    estimator: TokenEstimator,
}

impl PromptCompiler {
    pub fn new(estimator: TokenEstimator) -> Self {
        Self { estimator }
    }

    pub fn compile(&self, request: CompileRequest<'_>) -> Result<CompiledPrompt, CompileError> {
        let system_prompt = compile_system_prompt(&request);
        let system_tokens = u64::from(self.estimator.estimate(&system_prompt));
        if system_tokens > u64::from(request.token_budget) {
            return Err(CompileError::SystemPromptExceeds {
                required: system_tokens,
                budget: request.token_budget,
            });
        }
        let available = u64::from(request.token_budget) - system_tokens;
        let available =
            u32::try_from(available).map_err(|_| CompileError::SystemPromptExceeds {
                required: system_tokens,
                budget: request.token_budget,
            })?;
        let budget = ContextBudget::new(self.estimator).trim(request.context_items, available)?;
        Ok(CompiledPrompt {
            system_prompt,
            context_items: budget.items,
            system_tokens: u32::try_from(system_tokens).map_err(|_| {
                CompileError::SystemPromptExceeds {
                    required: system_tokens,
                    budget: request.token_budget,
                }
            })?,
            context_tokens: budget.tokens,
            dropped: budget.dropped,
        })
    }
}

fn compile_system_prompt(request: &CompileRequest<'_>) -> String {
    let mut output = String::new();
    append_profile(&mut output, request.profile);
    append_model(&mut output, request.model);
    append_workspace(&mut output, request.workspace);
    append_repository_instructions(&mut output, request.workspace);
    append_skills(&mut output, request.skills);
    if !request.model.supports_tools
        && let Some(guidance) = request
            .tool_guidance
            .filter(|value| !value.trim().is_empty())
    {
        section(&mut output, "Tool Guidance", guidance);
    }
    output
}

fn append_profile(output: &mut String, profile: &AgentProfile) {
    let mut body = format!("id: {}", profile.id);
    if let Some(persona) = &profile.persona {
        let _ = write!(body, "\npersona: {persona}");
    }
    if let Some(style) = &profile.communication_style {
        let _ = write!(body, "\ncommunication style: {style}");
    }
    let _ = write!(
        body,
        "\nworking style: prefer existing libraries={}, avoid reinventing wheels={}, inspect before editing={}, run tests after changes={}, architecture first={}\ncoding: avoid unnecessary abstraction={}, prefer small reviewable changes={}",
        profile.working_style.prefer_existing_libraries,
        profile.working_style.avoid_reinventing_wheels,
        profile.working_style.inspect_before_editing,
        profile.working_style.run_tests_after_changes,
        profile.working_style.architecture_first,
        profile.coding.avoid_unnecessary_abstraction,
        profile.coding.prefer_small_reviewable_changes,
    );
    if let Some(policy) = &profile.reasoning_policy {
        let _ = write!(body, "\nreasoning policy: {policy}");
    }
    if let Some(policy) = &profile.privacy_policy {
        let _ = write!(body, "\nprivacy policy: {policy}");
    }
    if !profile.tool_preferences.is_empty() {
        let _ = write!(
            body,
            "\ntool preferences: {}",
            profile.tool_preferences.join(", ")
        );
    }
    if !profile.default_skills.is_empty() {
        let _ = write!(
            body,
            "\ndefault skills: {}",
            profile.default_skills.join(", ")
        );
    }
    section(output, "Agent Profile", &body);
}

fn append_model(output: &mut String, model: &ModelCapabilities) {
    section(
        output,
        "Model Capabilities",
        &format!(
            "provider/model: {}\ntier: {:?}\nlocality: {:?}\nnative tools: {}\nJSON: {}\nthinking: {}\ncontext window: {}",
            model.key(),
            model.tier,
            model.locality,
            model.supports_tools,
            model.supports_json,
            model.supports_thinking,
            model.context_window,
        ),
    );
}

fn append_workspace(output: &mut String, workspace: &WorkspaceProfile) {
    section(
        output,
        "Workspace Profile",
        &format!(
            "project type: {:?}\nlanguages: {}\nframeworks: {}\nbuild commands: {}\ntest commands: {}\nconventions: {}",
            workspace.project_type,
            join_or_none(&workspace.languages),
            join_or_none(&workspace.frameworks),
            join_or_none(&workspace.build_commands),
            join_or_none(&workspace.test_commands),
            join_or_none(&workspace.conventions),
        ),
    );
}

fn append_repository_instructions(output: &mut String, workspace: &WorkspaceProfile) {
    let body = workspace
        .repository_instructions
        .iter()
        .map(|instruction| format!("### {}\n{}", instruction.path, instruction.content.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    section(output, "Repository Instructions", &body);
}

fn append_skills(output: &mut String, skills: &[Skill]) {
    let mut skills: Vec<_> = skills.iter().collect();
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    let body = skills
        .into_iter()
        .map(|skill| format!("### {}\n{}", skill.name, skill.instructions.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    section(output, "Matched Skills", &body);
}

fn section(output: &mut String, heading: &str, body: &str) {
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    let _ = write!(output, "## {heading}\n{}", body.trim());
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".into()
    } else {
        values.join(", ")
    }
}
