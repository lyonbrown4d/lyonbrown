use crate::{plan_command::WorkflowCommand as C, plan_view};
use agent_client::AgentClient;
use agent_protocol::v1;
use anyhow::{Context, Result};
pub(crate) async fn execute(
  client: &AgentClient,
  session: Option<&str>,
  command: C,
) -> Result<(String, String)> {
  let session = session
    .filter(|id| !id.is_empty())
    .context("Select a session first")?;
  let title = "Plans and changes".into();
  let detail = match command {
    C::Plans { after } => {
      let page = client
        .list_plans(v1::ListWorkflowPlansRequest {
          session_id: session.into(),
          run_id: String::new(),
          after_id: after,
          limit: 50,
        })
        .await?;
      format!(
        "{}\nNext cursor: {}",
        page
          .plans
          .iter()
          .map(plan_view::plan)
          .collect::<Vec<_>>()
          .join("\n\n"),
        page.next_after
      )
    }
    C::Plan(id) => plan_view::plan(&client.get_plan(session, &id).await?),
    C::Changes { after } => {
      let page = client
        .list_changes(v1::ListWorkflowChangesRequest {
          session_id: session.into(),
          after_id: after,
          limit: 50,
        })
        .await?;
      format!(
        "{}\nNext cursor: {}",
        page
          .changes
          .iter()
          .map(plan_view::change)
          .collect::<Vec<_>>()
          .join("\n\n"),
        page.next_after
      )
    }
    C::Change(id) => plan_view::change(&client.get_change(session, &id).await?),
    C::Apply {
      id,
      revision,
      digest,
    } => {
      let preview = client.get_change(session, &id).await?;
      anyhow::ensure!(
        preview.revision == revision && preview.preview_sha256 == digest,
        "Checkpoint changed; inspect the exact current diff before applying"
      );
      let epoch = client
        .runtime_status(session)
        .await?
        .session
        .context("Session unavailable")?
        .owner_epoch;
      let changed = client
        .apply_change(v1::ApplyWorkflowChangeRequest {
          session_id: session.into(),
          run_id: preview.run_id,
          request_id: new_id(),
          change_id: id,
          owner_epoch: epoch,
          expected_revision: revision,
          preview_sha256: digest,
          action_id: new_id(),
        })
        .await?;
      plan_view::change(&changed)
    }
    C::Undo(id) => {
      let source = client.get_change(session, &id).await?;
      let epoch = client
        .runtime_status(session)
        .await?
        .session
        .context("Session unavailable")?
        .owner_epoch;
      let preview = client
        .preview_undo(v1::PreviewWorkflowUndoRequest {
          session_id: session.into(),
          run_id: source.run_id,
          owner_epoch: epoch,
          request_id: new_id(),
          change_id: id,
        })
        .await?;
      format!(
        "{}\nReview this reverse preview; /changes apply {} {} {} approves only this reviewed preview.",
        plan_view::change(&preview),
        preview.id,
        preview.revision,
        preview.preview_sha256
      )
    }
    command => {
      let run_id = match &command {
        C::Step { plan, .. } | C::Verify { plan, .. } => {
          client.get_plan(session, plan).await?.run_id
        }
        C::Create { .. } => client
          .list_runs(session)
          .await?
          .last()
          .map(|run| run.id.clone())
          .context("Create a task first so the plan has a persisted run binding")?,
        _ => anyhow::bail!("unsupported plan command"),
      };
      use v1::mutate_workflow_plan_request::Change;
      let (plan_id, expected_revision, change) = match command {
        C::Create {
          title,
          step,
          acceptance,
        } => (
          new_id(),
          0,
          Change::Create(v1::WorkflowPlanDraft {
            title,
            steps: vec![v1::WorkflowStepDraft {
              id: new_id(),
              title: step,
              dependencies: vec![],
              acceptance: vec![acceptance],
            }],
          }),
        ),
        C::Step {
          plan,
          revision,
          step,
          state,
          report,
          override_reason,
        } => (
          plan,
          revision,
          Change::UpdateStep(v1::UpdateWorkflowStep {
            step_id: step,
            state: state as i32,
            reported_result: report,
            verification_refs: vec![],
            user_override: override_reason,
          }),
        ),
        C::Verify {
          plan,
          revision,
          step,
          operation,
        } => (
          plan,
          revision,
          Change::VerifyStep(v1::VerifyWorkflowStep {
            step_id: step,
            operation_id: operation,
          }),
        ),
        _ => anyhow::bail!("unsupported plan command"),
      };
      plan_view::plan(
        &client
          .mutate_plan(v1::MutateWorkflowPlanRequest {
            session_id: session.into(),
            run_id,
            request_id: new_id(),
            plan_id,
            expected_revision,
            change: Some(change),
          })
          .await?,
      )
    }
  };
  Ok((title, detail))
}
fn new_id() -> String {
  uuid::Uuid::now_v7().to_string()
}
