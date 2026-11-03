//! Assemble tools only after the decision plan and session mode have narrowed authority.
use crate::Daemon;
use agent_core::{Run, Session, interaction::InteractionMode};
use agent_decision::ExecutionPlan;
use agent_runtime::RuntimeTool;
use std::{collections::BTreeSet, sync::Arc};
pub(crate) struct ToolScope<'a> {
  pub run: &'a Run,
  pub session: &'a Session,
  pub plan: &'a ExecutionPlan,
  pub permitted_tools: Option<&'a BTreeSet<String>>,
  pub browser: bool,
  pub computer: Vec<Arc<dyn RuntimeTool>>,
  pub delegation_targets: BTreeSet<String>,
  pub mode: InteractionMode,
}
impl Daemon {
  pub(crate) async fn execution_tools(
    &self,
    scope: ToolScope<'_>,
  ) -> anyhow::Result<(Vec<Arc<dyn RuntimeTool>>, String)> {
    let ToolScope {
      run,
      session,
      plan,
      permitted_tools,
      browser,
      computer,
      delegation_targets,
      mode,
    } = scope;
    let mut tools =
      crate::tools::workspace_tools(self.clone(), run, &session.workspace, session.owner_epoch)?;
    let allowed_names = plan
      .allowed_tools
      .iter()
      .map(|tool| tool.name().to_owned())
      .collect();
    tools.extend(crate::memory::tools(
      self.clone(),
      run,
      session,
      &allowed_names,
    ));
    tools.extend(crate::planning::tools(
      self.clone(),
      run,
      session,
      &allowed_names,
    ));
    tools.extend(crate::changes::tools(self.clone(), run, session)?);
    let (skill_prompt, skill_tool) = crate::skill_runtime::prepare(self, run, session).await?;
    if let Some(tool) = skill_tool {
      tools.push(tool);
    }
    if let Some(tool) = self.mcp.tool(self.clone(), run, session.owner_epoch) {
      tools.push(tool);
    }
    if browser {
      tools.extend(crate::browser::tools(self.clone(), run, session));
    }
    tools.extend(crate::commands::tools(self.clone(), run, session)?);
    tools.extend(computer);
    if plan.execution_mode == agent_decision::ExecutionMode::DistributedAllowed {
      let targets = delegation_targets
        .into_iter()
        .filter(|target| plan.target_hint.as_ref().is_none_or(|hint| hint == target))
        .collect();
      let scoped_capabilities = [
        (agent_decision::Tool::ReadFile, "workspace.read"),
        (agent_decision::Tool::ListFiles, "workspace.read"),
        (agent_decision::Tool::SearchFiles, "workspace.read"),
        (agent_decision::Tool::WriteFile, "workspace.write.approval"),
      ]
      .into_iter()
      .filter(|(tool, _)| plan.allowed_tools.contains(tool))
      .map(|(_, capability)| capability.to_owned())
      .collect::<std::collections::BTreeSet<_>>()
      .into_iter()
      .collect();
      tools.extend(crate::delegation::tools(
        self.clone(),
        run,
        session,
        targets,
        scoped_capabilities,
      ));
    }
    tools.retain(|tool| {
      mode.permits(&tool.definition().name)
        && permitted_tools.is_none_or(|permitted| permitted.contains(&tool.definition().name))
        && plan
          .allowed_tools
          .iter()
          .any(|allowed| allowed.name() == tool.definition().name)
    });
    Ok((tools, skill_prompt))
  }
}
