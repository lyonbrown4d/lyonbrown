use super::{Reply, WorkflowPane};
use agent_protocol::v1;
use gpui_kit::*;
impl WorkflowPane {
  pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.request(
      false,
      async move {
        let (plans, changes) = tokio::try_join!(
          client.list_plans(v1::ListWorkflowPlansRequest {
            session_id: session.clone(),
            run_id: String::new(),
            after_id: String::new(),
            limit: 25
          }),
          client.list_changes(v1::ListWorkflowChangesRequest {
            session_id: session,
            after_id: String::new(),
            limit: 25
          })
        )?;
        Ok(Reply::Inventory(plans, changes))
      },
      cx,
    );
  }
  pub(super) fn more(&mut self, plans: bool, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    let after = if plans {
      self.plan_after.clone()
    } else {
      self.change_after.clone()
    };
    if after.is_empty() {
      return;
    }
    self.request(
      false,
      async move {
        Ok(if plans {
          Reply::Plans(
            client
              .list_plans(v1::ListWorkflowPlansRequest {
                session_id: session,
                run_id: String::new(),
                after_id: after,
                limit: 25,
              })
              .await?,
          )
        } else {
          Reply::Changes(
            client
              .list_changes(v1::ListWorkflowChangesRequest {
                session_id: session,
                after_id: after,
                limit: 25,
              })
              .await?,
          )
        })
      },
      cx,
    );
  }
  pub(super) fn inspect(&mut self, id: String, plan: bool, cx: &mut Context<Self>) {
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    self.request(
      false,
      async move {
        Ok(if plan {
          Reply::Plan(client.get_plan(&session, &id).await?)
        } else {
          Reply::Change(client.get_change(&session, &id).await?)
        })
      },
      cx,
    );
  }
  pub(super) fn undo(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(session), Some(change)) = (
      self.client.clone(),
      self.session.clone(),
      self.change.clone(),
    ) else {
      return;
    };
    self.request(
      true,
      async move {
        let epoch = client
          .runtime_status(&session)
          .await?
          .session
          .ok_or_else(|| anyhow::anyhow!("Session unavailable"))?
          .owner_epoch;
        Ok(Reply::Change(
          client
            .preview_undo(v1::PreviewWorkflowUndoRequest {
              session_id: session,
              run_id: change.run_id,
              owner_epoch: epoch,
              request_id: uuid::Uuid::now_v7().to_string(),
              change_id: change.id,
            })
            .await?,
        ))
      },
      cx,
    );
  }
  pub(super) fn apply(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(session), Some(change)) = (
      self.client.clone(),
      self.session.clone(),
      self.change.clone(),
    ) else {
      return;
    };
    if !change.detail_included {
      return;
    }
    self.message = "Applying only the reviewed revision and hash. The daemon checks ownership and current file revisions again.".into();
    self.request(
      true,
      async move {
        let epoch = client
          .runtime_status(&session)
          .await?
          .session
          .ok_or_else(|| anyhow::anyhow!("Session unavailable"))?
          .owner_epoch;
        Ok(Reply::Change(
          client
            .apply_change(v1::ApplyWorkflowChangeRequest {
              session_id: session,
              run_id: change.run_id,
              owner_epoch: epoch,
              request_id: uuid::Uuid::now_v7().to_string(),
              change_id: change.id,
              expected_revision: change.revision,
              preview_sha256: change.preview_sha256,
              action_id: uuid::Uuid::now_v7().to_string(),
            })
            .await?,
        ))
      },
      cx,
    );
  }
}
