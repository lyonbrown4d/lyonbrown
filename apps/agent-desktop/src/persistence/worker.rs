use crate::local_state::DesktopState;
use crate::tabs::ScopedUpdate;
use agent_client::local_state::LocalStateStore;
use std::{path::PathBuf, time::Duration};
use tokio::sync::mpsc::{Receiver, Sender};

pub(crate) type LatestSnapshot = std::sync::Arc<std::sync::Mutex<Option<(u64, DesktopState)>>>;

pub(crate) struct ActorConfig {
  pub(crate) root: PathBuf,
  pub(crate) endpoint: String,
  pub(crate) node_id: String,
  pub(crate) scope: String,
  pub(crate) generation: u64,
}

#[derive(Debug)]
pub(crate) enum Command {
  Save {
    scope: String,
    generation: u64,
    sequence: u64,
  },
  Stop {
    scope: String,
    generation: u64,
    sequence: u64,
    state: Option<Box<DesktopState>>,
    ack: Option<tokio::sync::oneshot::Sender<()>>,
  },
}

#[derive(Debug)]
pub(crate) enum ResultMessage {
  Loaded {
    scope: String,
    generation: u64,
    state: Option<Box<DesktopState>>,
  },
  Failed {
    scope: String,
    generation: u64,
    message: String,
  },
  Saved {
    scope: String,
    generation: u64,
    sequence: u64,
  },
  SaveFailed {
    scope: String,
    generation: u64,
    sequence: u64,
    message: String,
  },
}

pub(crate) fn start(
  runtime: std::sync::Arc<tokio::runtime::Runtime>,
  latest: LatestSnapshot,
  config: ActorConfig,
  previous: Option<tokio::sync::oneshot::Receiver<()>>,
  replies: async_channel::Sender<ScopedUpdate>,
) -> Sender<Command> {
  let (sender, receiver) = tokio::sync::mpsc::channel(1);
  runtime.spawn(async move {
    if let Some(previous) = previous {
      let _ = previous.await;
    }
    run(config, latest, replies, receiver).await;
  });
  sender
}

fn send_important(replies: async_channel::Sender<ScopedUpdate>, update: ScopedUpdate) {
  tokio::spawn(async move {
    let _ = replies.send(update).await;
  });
}

async fn run(
  config: ActorConfig,
  latest: LatestSnapshot,
  replies: async_channel::Sender<ScopedUpdate>,
  mut receiver: Receiver<Command>,
) {
  let ActorConfig {
    root,
    endpoint,
    node_id,
    scope,
    generation,
  } = config;
  let opened = tokio::task::spawn_blocking(move || {
    let store = LocalStateStore::new(root, "desktop", &endpoint, &node_id)?;
    let state = store.load::<DesktopState>()?;
    if let Some(state) = &state {
      state.validate()?;
    }
    anyhow::Ok((store, state))
  })
  .await;
  let (store, initial) = match opened {
    Ok(Ok(opened)) => opened,
    Ok(Err(error)) => {
      send_important(
        replies.clone(),
        ScopedUpdate::Persistence(ResultMessage::Failed {
          scope,
          generation,
          message: error.to_string(),
        }),
      );
      return;
    }
    Err(error) => {
      send_important(
        replies.clone(),
        ScopedUpdate::Persistence(ResultMessage::Failed {
          scope,
          generation,
          message: error.to_string(),
        }),
      );
      return;
    }
  };
  send_important(
    replies.clone(),
    ScopedUpdate::Persistence(ResultMessage::Loaded {
      scope: scope.clone(),
      generation,
      state: initial.map(Box::new),
    }),
  );

  let store = std::sync::Arc::new(store);
  let mut pending: Option<u64> = None;
  let mut release_ack = None;
  loop {
    let command = if pending.is_some() {
      tokio::select! {
        command = receiver.recv() => command,
        _ = tokio::time::sleep(Duration::from_millis(400)) => {
          if pending.take().is_some() {
            let snapshot = latest.lock().ok().and_then(|latest| latest.clone());
            if let Some((sequence, state)) = snapshot {
              write_latest(store.clone(), state, scope.clone(), generation, sequence, &replies).await;
            }
          }
          continue;
        }
      }
    } else {
      receiver.recv().await
    };
    match command {
      Some(Command::Save {
        scope: command_scope,
        generation: command_generation,
        sequence,
      }) => {
        if command_scope == scope && command_generation == generation {
          pending = Some(sequence);
        }
      }
      Some(Command::Stop {
        scope: command_scope,
        generation: command_generation,
        sequence,
        state,
        ack,
      }) => {
        if command_scope == scope && command_generation == generation {
          if let Some(state) = state {
            write_latest(
              store.clone(),
              *state,
              scope.clone(),
              generation,
              sequence,
              &replies,
            )
            .await;
          } else if pending.take().is_some()
            && let Some((sequence, state)) = latest.lock().ok().and_then(|latest| latest.clone())
          {
            write_latest(
              store.clone(),
              state,
              scope.clone(),
              generation,
              sequence,
              &replies,
            )
            .await;
          }
          release_ack = ack;
          break;
        }
      }
      None => {
        if pending.take().is_some()
          && let Some((sequence, state)) = latest.lock().ok().and_then(|latest| latest.clone())
        {
          write_latest(
            store.clone(),
            state,
            scope.clone(),
            generation,
            sequence,
            &replies,
          )
          .await;
        }
        break;
      }
    }
  }
  drop(store);
  if let Some(ack) = release_ack {
    let _ = ack.send(());
  }
}

async fn write_latest(
  store: std::sync::Arc<LocalStateStore>,
  state: DesktopState,
  scope: String,
  generation: u64,
  sequence: u64,
  replies: &async_channel::Sender<ScopedUpdate>,
) {
  if let Err(error) = state.validate() {
    let message = error.to_string();
    send_important(
      replies.clone(),
      ScopedUpdate::Persistence(ResultMessage::SaveFailed {
        scope,
        generation,
        sequence,
        message,
      }),
    );
    return;
  }
  match tokio::task::spawn_blocking(move || store.save(&state)).await {
    Ok(Ok(())) => {
      let _ = replies.try_send(ScopedUpdate::Persistence(ResultMessage::Saved {
        scope,
        generation,
        sequence,
      }));
    }
    Ok(Err(error)) => {
      let message = error.to_string();
      tracing::error!(scope, generation, error = %message, "desktop state save failed");
      send_important(
        replies.clone(),
        ScopedUpdate::Persistence(ResultMessage::SaveFailed {
          scope,
          generation,
          sequence,
          message,
        }),
      );
    }
    Err(error) => {
      let message = error.to_string();
      tracing::error!(scope, generation, error = %message, "desktop state save worker failed");
      send_important(
        replies.clone(),
        ScopedUpdate::Persistence(ResultMessage::SaveFailed {
          scope,
          generation,
          sequence,
          message,
        }),
      );
    }
  }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
