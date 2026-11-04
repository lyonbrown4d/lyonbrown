use agent_protocol::v1;
pub(crate) fn plan(plan: &v1::WorkflowPlan) -> String {
  let mut text = format!(
    "{} · {} · revision {}\nRun {}",
    plan.title, plan.id, plan.revision, plan.run_id
  );
  for step in &plan.steps {
    if let Some(definition) = &step.definition {
      text.push_str(&format!(
        "\n- {:?} {} · {}\n  acceptance: {}\n  reported: {}\n  verified refs: {}{}",
        v1::WorkflowStepState::try_from(step.state).ok(),
        definition.title,
        definition.id,
        definition.acceptance.join("; "),
        step.reported_result,
        step.verification_refs.join(", "),
        step
          .user_override
          .as_ref()
          .map(|reason| format!("\n  user override: {reason}"))
          .unwrap_or_default()
      ));
    }
  }
  if !plan.detail_included {
    text.push_str("\nUse /plan get ID for acceptance and verification details.");
  }
  for receipt in &plan.verifications {
    text.push_str(&format!(
      "\nHost verification {} · passed={} · {}",
      receipt.id, receipt.passed, receipt.summary
    ));
  }
  text
}
pub(crate) fn change(change: &v1::WorkflowChange) -> String {
  let mut text = format!(
    "{} · revision {} · {:?}\nDigest {}\nRun {}",
    change.id,
    change.revision,
    v1::WorkflowChangeState::try_from(change.state).ok(),
    change.preview_sha256,
    change.run_id
  );
  for file in &change.files {
    text.push_str(&format!("\n{}\n{}", file.path, file.unified_diff));
  }
  if !change.detail_included {
    text.push_str("\nUse /changes get ID to inspect the exact diff before applying.");
  }
  for receipt in &change.validations {
    text.push_str(&format!(
      "\nHost command {} · passed={} · exit={:?}",
      receipt.operation_id, receipt.passed, receipt.exit_code
    ));
  }
  text
}
