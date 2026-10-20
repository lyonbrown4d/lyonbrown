use super::state::{Reply, Workbench};

impl Workbench {
  pub(super) fn watch_workspace(&mut self) {
    if let Some(watch) = self.watch.take() {
      watch.abort();
    }
    let (Some(client), Some(session)) = (self.client.clone(), self.session.clone()) else {
      return;
    };
    let sender = self.sender.clone();
    let generation = self.generation;
    self.watch = Some(self.runtime.spawn(async move {
      let result = async {
        let mut events = client.watch_files(&session).await?;
        while let Some(event) = events.message().await? {
          if sender
            .send((generation, Reply::Changed(event.paths)))
            .await
            .is_err()
          {
            break;
          }
        }
        anyhow::Ok(())
      }
      .await;
      if let Err(error) = result {
        let _ = sender
          .send((
            generation,
            Reply::WatchError(format!(
              "Workspace watch disconnected: {error}. Refresh files to inspect current state."
            )),
          ))
          .await;
      }
    }));
  }
}
impl Drop for Workbench {
  fn drop(&mut self) {
    if let Some(watch) = self.watch.take() {
      watch.abort();
    }
  }
}
