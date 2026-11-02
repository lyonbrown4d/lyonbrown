use crate::local_state::DesktopState;
use crate::tabs::ScopedUpdate;
use agent_client::local_state::LocalStateStore;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};

pub(crate) use super::command::{ActorConfig, Command, LatestSnapshot, ResultMessage};

/// How a result reaches the bounded UI queue.
///
/// `Queue` waits for capacity, so results stay ordered and a burst cannot drop
/// them silently. The wait is bounded: a wedged UI must not keep the actor from
/// the `Stop` acknowledgement. The final flush is best-effort because it must
/// acknowledge the store release even when no one drains the queue any more.
#[derive(Clone, Copy)]
enum Delivery {
  Queue,
  BestEffort,
}

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(2);

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

async fn deliver(
  replies: &async_channel::Sender<ScopedUpdate>,
  update: ScopedUpdate,
  delivery: Delivery,
) {
  match delivery {
    Delivery::Queue => {
      let sent = tokio::time::timeout(DELIVERY_TIMEOUT, replies.send(update)).await;
      if !matches!(sent, Ok(Ok(()))) {
        tracing::warn!("desktop state result missed its delivery deadline");
      }
    }
    Delivery::BestEffort => {
      if replies.try_send(update).is_err() {
        tracing::warn!("desktop state result dropped while the final flush released the store");
      }
    }
  }
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
      let message = error.to_string();
      deliver(
        &replies,
        ScopedUpdate::Persistence(ResultMessage::Failed {
          scope,
          generation,
          message,
        }),
        Delivery::Queue,
      )
      .await;
      return;
    }
    Err(error) => {
      let message = error.to_string();
      deliver(
        &replies,
        ScopedUpdate::Persistence(ResultMessage::Failed {
          scope,
          generation,
          message,
        }),
        Delivery::Queue,
      )
      .await;
      return;
    }
  };
  deliver(
    &replies,
    ScopedUpdate::Persistence(ResultMessage::Loaded {
      scope: scope.clone(),
      generation,
      state: initial.map(Box::new),
    }),
    Delivery::Queue,
  )
  .await;

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
              write_latest(
                store.clone(),
                state,
                scope.clone(),
                generation,
                sequence,
                &replies,
                Delivery::Queue,
              )
              .await;
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
              Delivery::BestEffort,
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
              Delivery::BestEffort,
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
            Delivery::Queue,
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
  delivery: Delivery,
) {
  if let Err(error) = state.validate() {
    let message = error.to_string();
    deliver(
      replies,
      ScopedUpdate::Persistence(ResultMessage::SaveFailed {
        scope,
        generation,
        sequence,
        message,
      }),
      delivery,
    )
    .await;
    return;
  }
  match tokio::task::spawn_blocking(move || store.save(&state)).await {
    Ok(Ok(())) => {
      deliver(
        replies,
        ScopedUpdate::Persistence(ResultMessage::Saved {
          scope,
          generation,
          sequence,
        }),
        delivery,
      )
      .await;
    }
    Ok(Err(error)) => {
      let message = error.to_string();
      tracing::error!(scope, generation, error = %message, "desktop state save failed");
      deliver(
        replies,
        ScopedUpdate::Persistence(ResultMessage::SaveFailed {
          scope,
          generation,
          sequence,
          message,
        }),
        delivery,
      )
      .await;
    }
    Err(error) => {
      let message = error.to_string();
      tracing::error!(scope, generation, error = %message, "desktop state save worker failed");
      deliver(
        replies,
        ScopedUpdate::Persistence(ResultMessage::SaveFailed {
          scope,
          generation,
          sequence,
          message,
        }),
        delivery,
      )
      .await;
    }
  }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
