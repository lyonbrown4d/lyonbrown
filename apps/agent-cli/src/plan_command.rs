use agent_protocol::v1;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowCommand {
  Plans {
    after: String,
  },
  Plan(String),
  Create {
    title: String,
    step: String,
    acceptance: String,
  },
  Step {
    plan: String,
    revision: u64,
    step: String,
    state: v1::WorkflowStepState,
    report: String,
    override_reason: Option<String>,
  },
  Verify {
    plan: String,
    revision: u64,
    step: String,
    operation: String,
  },
  Changes {
    after: String,
  },
  Change(String),
  Undo(String),
  Apply {
    id: String,
    revision: u64,
    digest: String,
  },
}
pub(crate) const USAGE: &str = "/plan [list [AFTER]|get ID|create TITLE STEP ACCEPTANCE|step PLAN REV STEP STATE REPORT|complete PLAN REV STEP REASON|verify PLAN REV STEP OPERATION] · /changes [list [AFTER]|get ID|apply ID REVISION HASH] · /undo CHANGE_ID (build reverse preview; apply still requires approval)";
pub(crate) fn parse(root: &str, args: &[String]) -> Result<WorkflowCommand, String> {
  let revision = |value: &str| {
    value
      .parse::<u64>()
      .map_err(|_| "revision must be an integer".to_owned())
  };
  match (root, args) {
    ("/plan", []) => Ok(WorkflowCommand::Plans {
      after: String::new(),
    }),
    ("/plan", [verb]) if verb == "list" => Ok(WorkflowCommand::Plans {
      after: String::new(),
    }),
    ("/plan", [verb, after]) if verb == "list" => Ok(WorkflowCommand::Plans {
      after: after.clone(),
    }),
    ("/plan", [verb, id]) if verb == "get" => Ok(WorkflowCommand::Plan(id.clone())),
    ("/plan", [verb, title, step, acceptance]) if verb == "create" => Ok(WorkflowCommand::Create {
      title: title.clone(),
      step: step.clone(),
      acceptance: acceptance.clone(),
    }),
    ("/plan", [verb, plan, rev, step, state, report]) if verb == "step" => {
      let state = match state.as_str() {
        "pending" => v1::WorkflowStepState::Pending,
        "running" => v1::WorkflowStepState::Running,
        "blocked" => v1::WorkflowStepState::Blocked,
        "skipped" => v1::WorkflowStepState::Skipped,
        _ => {
          return Err(
            "Use /plan complete for an explicit completion override; verification is separate."
              .into(),
          );
        }
      };
      Ok(WorkflowCommand::Step {
        plan: plan.clone(),
        revision: revision(rev)?,
        step: step.clone(),
        state,
        report: report.clone(),
        override_reason: None,
      })
    }
    ("/plan", [verb, plan, rev, step, reason]) if verb == "complete" => Ok(WorkflowCommand::Step {
      plan: plan.clone(),
      revision: revision(rev)?,
      step: step.clone(),
      state: v1::WorkflowStepState::Completed,
      report: String::new(),
      override_reason: Some(reason.clone()),
    }),
    ("/plan", [verb, plan, rev, step, operation]) if verb == "verify" => {
      Ok(WorkflowCommand::Verify {
        plan: plan.clone(),
        revision: revision(rev)?,
        step: step.clone(),
        operation: operation.clone(),
      })
    }
    ("/changes", []) => Ok(WorkflowCommand::Changes {
      after: String::new(),
    }),
    ("/changes", [verb]) if verb == "list" => Ok(WorkflowCommand::Changes {
      after: String::new(),
    }),
    ("/changes", [verb, after]) if verb == "list" => Ok(WorkflowCommand::Changes {
      after: after.clone(),
    }),
    ("/changes", [verb, id]) if verb == "get" => Ok(WorkflowCommand::Change(id.clone())),
    ("/changes", [verb, id, rev, digest]) if verb == "apply" => Ok(WorkflowCommand::Apply {
      id: id.clone(),
      revision: revision(rev)?,
      digest: digest.clone(),
    }),
    ("/undo", [id]) => Ok(WorkflowCommand::Undo(id.clone())),
    _ => Err(USAGE.into()),
  }
}
