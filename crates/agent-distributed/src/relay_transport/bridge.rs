use std::sync::Arc;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  sync::{Semaphore, mpsc},
  task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use super::{Command, RelayIo};

pub(super) const CHUNK: usize = 16 * 1024;
pub(super) const WINDOW: usize = 4;

/// Pumps only move bounded byte buffers; the session actor owns Noise state and ordering.
pub(super) fn start(
  id: String,
  generation: u64,
  commands: mpsc::Sender<Command>,
  stop: CancellationToken,
  tasks: &mut JoinSet<()>,
) -> (RelayIo, mpsc::Sender<Vec<u8>>, Arc<Semaphore>) {
  let (application, bridge) = tokio::io::duplex(64 * 1024);
  let (mut reader, mut writer) = tokio::io::split(bridge);
  let (incoming, mut received) = mpsc::channel::<Vec<u8>>(WINDOW);
  let credit = Arc::new(Semaphore::new(WINDOW));
  let send_credit = credit.clone();
  let application_io = RelayIo {
    inner: application,
    id: id.clone(),
    generation,
    commands: commands.clone(),
    stop: stop.clone(),
  };
  let send_id = id.clone();
  let send_commands = commands.clone();
  let send_stop = stop.clone();
  tasks.spawn(async move {
    let mut buffer = vec![0; CHUNK];
    loop {
      let permit = tokio::select! {
        _ = send_stop.cancelled() => return,
        permit = send_credit.acquire() => match permit { Ok(permit) => permit, Err(_) => return },
      };
      let read = tokio::select! {
        _ = send_stop.cancelled() => return,
        read = reader.read(&mut buffer) => read,
      };
      let (command, done) = match read {
        Ok(0) => (
          Command::Fin {
            id: send_id.clone(),
            generation,
          },
          true,
        ),
        Ok(size) => (
          Command::Data {
            id: send_id.clone(),
            generation,
            bytes: buffer[..size].to_vec(),
          },
          false,
        ),
        Err(_) => (
          Command::Abort {
            id: send_id.clone(),
            generation,
          },
          true,
        ),
      };
      if !done {
        permit.forget();
      } else {
        drop(permit);
      }
      tokio::select! {
        _ = send_stop.cancelled() => return,
        sent = send_commands.send(command) => if sent.is_err() { return },
      }
      if done {
        return;
      }
    }
  });
  tasks.spawn(async move {
    loop {
      let bytes = tokio::select! {
        _ = stop.cancelled() => return,
        bytes = received.recv() => bytes,
      };
      let Some(bytes) = bytes else { break };
      let result = tokio::select! {
        _ = stop.cancelled() => return,
        result = writer.write_all(&bytes) => result,
      };
      if result.is_err() {
        let _ = commands.try_send(Command::Abort { id, generation });
        return;
      }
      tokio::select! {
        _ = stop.cancelled() => return,
        result = commands.send(Command::Credit { id: id.clone(), generation }) => if result.is_err() { return },
      }
    }
    tokio::select! {
      _ = stop.cancelled() => {},
      _ = async {
        let _ = writer.shutdown().await;
        let _ = commands.send(Command::Drained { id, generation }).await;
      } => {},
    }
  });
  (application_io, incoming, credit)
}
