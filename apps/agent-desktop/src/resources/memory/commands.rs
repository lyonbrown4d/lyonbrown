use super::{Completion, MemoryPanel, Reply};
use agent_protocol::v1;
use gpui_kit::*;
impl MemoryPanel {
  pub(super) fn request<F>(&mut self, mutation: bool, future: F, cx: &mut Context<Self>)
  where
    F: std::future::Future<Output = anyhow::Result<Reply>> + Send + 'static,
  {
    if self.busy || self.pending || self.client.is_none() {
      return;
    }
    self.busy = true;
    self.pending = mutation;
    self.message.clear();
    let sender = self.sender.clone();
    let generation = self.generation;
    self.runtime.spawn(async move {
      let result = future.await;
      let _ = sender
        .send(Completion {
          generation,
          mutation,
          result,
        })
        .await;
    });
    cx.notify();
  }
  pub(in crate::resources) fn refresh(&mut self, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    self.request(
      false,
      async move {
        let (items, status, rebuilds) = tokio::try_join!(
          client.list_memories(v1::ListMemoriesRequest {
            session_id: session.clone(),
            states: vec![],
            after_id: String::new(),
            limit: 50
          }),
          client.memory_status(),
          client.memory_rebuilds(&session, "", 20),
        )?;
        Ok(Reply::Snapshot(items, status, rebuilds))
      },
      cx,
    );
  }
  pub(super) fn more(&mut self, cx: &mut Context<Self>) {
    if self.next_cursor.is_empty() {
      return;
    }
    let Some(client) = self.client.clone() else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    let after = self.next_cursor.clone();
    self.request(
      false,
      async move {
        Ok(Reply::List(
          client
            .list_memories(v1::ListMemoriesRequest {
              session_id: session,
              states: vec![],
              after_id: after,
              limit: 50,
            })
            .await?,
          true,
        ))
      },
      cx,
    );
  }
  pub(super) fn search(&mut self, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    let query = self.query.read(cx).value().to_string();
    if query.trim().is_empty() {
      self.refresh(cx);
      return;
    }
    self.request(
      false,
      async move {
        let result = client.search_memories(&session, &query, 50).await?;
        Ok(Reply::List(
          v1::Memories {
            items: result
              .items
              .into_iter()
              .filter_map(|item| item.memory)
              .collect(),
            next_cursor: String::new(),
          },
          false,
        ))
      },
      cx,
    );
  }
  pub(super) fn save(&mut self, state: v1::MemoryState, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let content = self.content.read(cx).value().to_string();
    if content.trim().is_empty() || content.len() > 16_384 {
      self.message = "Enter 1–16,384 bytes of memory content.".into();
      cx.notify();
      return;
    }
    let selected = self.selected.as_ref();
    let request = v1::PutMemoryRequest {
      session_id: self.session.clone().unwrap_or_default(),
      id: selected.map_or_else(|| self.draft_id.clone(), |item| item.id.clone()),
      request_id: uuid::Uuid::now_v7().to_string(),
      scope_kind: selected
        .and_then(|item| item.scope.as_ref())
        .map_or(self.draft_scope as i32, |scope| scope.kind),
      kind: selected.map_or(self.draft_kind as i32, |item| item.kind),
      state: state as i32,
      content,
      expected_revision: selected.map_or(0, |item| item.revision),
      expires_at_ms: selected.and_then(|item| item.expires_at_ms),
    };
    self.request(
      true,
      async move {
        client.put_memory(request).await?;
        Ok(Reply::Changed(
          "Memory saved. Refresh shows the authoritative revision.".into(),
        ))
      },
      cx,
    );
  }
  pub(super) fn forget(&mut self, cx: &mut Context<Self>) {
    let (Some(client), Some(item)) = (self.client.clone(), self.selected.clone()) else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    self.request(
      true,
      async move {
        client
          .forget_memory(&session, &item.id, item.revision)
          .await?;
        Ok(Reply::Forgotten)
      },
      cx,
    );
  }
  pub(super) fn retry(&mut self, job: v1::MemoryJob, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    self.request(
      true,
      async move {
        client
          .retry_memory_job(&session, &job.memory_id, &job.id)
          .await?;
        Ok(Reply::Changed("Index job explicitly queued again.".into()))
      },
      cx,
    );
  }
  pub(super) fn rebuild(&mut self, cx: &mut Context<Self>) {
    let Some(client) = self.client.clone() else {
      return;
    };
    let session = self.session.clone().unwrap_or_default();
    self.request(
      true,
      async move {
        client
          .start_memory_rebuild(v1::StartMemoryRebuildRequest {
            session_id: session,
            request_id: uuid::Uuid::now_v7().to_string(),
            force: true,
          })
          .await?;
        Ok(Reply::Changed(
          "Rebuild queued for the currently accessible scopes.".into(),
        ))
      },
      cx,
    );
  }
}
