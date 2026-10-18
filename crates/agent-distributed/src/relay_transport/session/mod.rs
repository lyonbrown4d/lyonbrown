mod frames;
mod links;

use std::{
  collections::{HashMap, HashSet, VecDeque},
  time::Duration,
};

use anyhow::{Result, ensure};
use tokio::{
  sync::{mpsc, oneshot},
  task::JoinSet,
  time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::{Command, RelayIo, RelayRuntime};
use crate::{
  peers::PeerManager,
  relay_client::RelayConnection,
  secure_link::{LinkHandshake, SecureLink},
};

const MAX_LINKS: usize = 32;
const MAX_PENDING: usize = 8;
const HANDSHAKE: u64 = u64::MAX;
const DATA: u8 = 0;
const FIN: u8 = 1;
const ABORT: u8 = 2;
const CREDIT: u8 = 3;
const VERSION: u8 = 1;

#[derive(Clone, Default)]
pub(super) struct Recent {
  ids: HashSet<String>,
  order: VecDeque<String>,
}
impl Recent {
  fn insert(&mut self, id: String) {
    if self.ids.insert(id.clone()) {
      self.order.push_back(id);
    }
    while self.order.len() > 1024 {
      if let Some(id) = self.order.pop_front() {
        self.ids.remove(&id);
      }
    }
  }
}

enum Stage {
  Pending {
    handshake: Option<Box<LinkHandshake>>,
    reply: Option<oneshot::Sender<Result<RelayIo>>>,
  },
  Active {
    secure: Box<SecureLink>,
    incoming: Option<mpsc::Sender<Vec<u8>>>,
    credit: std::sync::Arc<tokio::sync::Semaphore>,
    outstanding: usize,
    received: usize,
    local_fin: bool,
    remote_fin: bool,
    drained: bool,
  },
}
struct Link {
  id: String,
  peer: String,
  fingerprint: String,
  activity: Instant,
  stop: CancellationToken,
  stage: Stage,
}

pub(super) struct Session {
  manager: PeerManager,
  incoming_frames: tonic::Streaming<agent_protocol::v1::RelayEnvelope>,
  outgoing: mpsc::Sender<agent_protocol::v1::RelayEnvelope>,
  outbound_failure: oneshot::Receiver<tonic::Status>,
  generation: u64,
  control: String,
  control_fingerprint: String,
  commands: mpsc::Sender<Command>,
  incoming: mpsc::Sender<std::io::Result<RelayIo>>,
  links: HashMap<String, Link>,
  recent: Recent,
  stop: CancellationToken,
  tasks: JoinSet<()>,
}
impl Session {
  pub fn new(
    runtime: &RelayRuntime,
    connection: RelayConnection,
    generation: u64,
    control_fingerprint: String,
    stop: CancellationToken,
  ) -> Self {
    let (sender, incoming_frames) = connection.into_parts();
    let (outgoing, mut frames) = mpsc::channel(super::QUEUE);
    let (failed, outbound_failure) = oneshot::channel();
    let outbound_stop = stop.clone();
    let mut tasks = JoinSet::new();
    tasks.spawn(async move {
      loop {
        let frame = tokio::select! {
          _ = outbound_stop.cancelled() => return,
          frame = frames.recv() => match frame { Some(frame) => frame, None => return },
        };
        let result = tokio::select! {
          _ = outbound_stop.cancelled() => return,
          result = sender.send(frame) => result,
        };
        if let Err(error) = result {
          let _ = failed.send(error);
          return;
        }
      }
    });
    Self {
      manager: runtime.manager.clone(),
      incoming_frames,
      outgoing,
      outbound_failure,
      generation,
      control: runtime.control.clone(),
      control_fingerprint,
      commands: runtime.handle.commands.clone(),
      incoming: runtime.incoming.clone(),
      links: HashMap::new(),
      recent: runtime.recent.clone(),
      stop,
      tasks,
    }
  }

  pub async fn run(&mut self, commands: &mut mpsc::Receiver<Command>) -> Result<()> {
    let result = self.drive(commands).await;
    if self.stop.is_cancelled() {
      Ok(())
    } else {
      result
    }
  }

  async fn drive(&mut self, commands: &mut mpsc::Receiver<Command>) -> Result<()> {
    let mut check = tokio::time::interval(Duration::from_millis(100));
    loop {
      tokio::select! {
        _ = self.stop.cancelled() => return Ok(()),
        _ = self.incoming.closed() => anyhow::bail!("relay incoming server closed"),
        command = commands.recv(), if self.outgoing.capacity() > 0 => match command {
          Some(command) => {
            if let Err(error) = self.command(command) {
              if self.stop.is_cancelled() { return Ok(()) }
              tracing::warn!(operation = "relay.bridge", outcome = "generation_failed",
                reason = %error, "relay byte bridge command failed");
              return Err(error);
            }
          },
          None => return Ok(()),
        },
        capacity = self.outgoing.clone().reserve_owned(), if self.outgoing.capacity() == 0 => { drop(capacity?); },
        failure = &mut self.outbound_failure => {
          return Err(failure.unwrap_or_else(|_| tonic::Status::unavailable("relay outbound pump closed")).into());
        },
        frame = self.incoming_frames.message() => match frame? {
          Some(frame) => self.frame(frame)?,
          None => anyhow::bail!("relay generation disconnected"),
        },
        _ = check.tick() => {
          ensure!(self.paired(&self.control)? == self.control_fingerprint,
            "relay control pairing changed");
          self.expire()?;
        },
        result = self.tasks.join_next(), if !self.tasks.is_empty() => {
          if let Some(result) = result { result?; }
        },
      }
    }
  }

  fn queue(&self, frame: agent_protocol::v1::RelayEnvelope) -> Result<()> {
    self.outgoing.try_send(frame).map_err(|error| match error {
      mpsc::error::TrySendError::Full(_) => {
        anyhow::anyhow!("relay outbound admission invariant failed")
      }
      mpsc::error::TrySendError::Closed(_) => anyhow::anyhow!("relay outbound pump closed"),
    })
  }

  fn command(&mut self, command: Command) -> Result<()> {
    match command {
      Command::Open {
        peer,
        generation,
        reply,
      } => {
        if generation != self.generation {
          let _ = reply.send(Err(anyhow::anyhow!("relay generation changed before open")));
          return Ok(());
        }
        self.open(peer, reply)
      }
      Command::Data {
        id,
        generation,
        bytes,
      } if generation == self.generation => {
        ensure!(
          bytes.len() <= super::bridge::CHUNK,
          "relay bridge chunk exceeds limit"
        );
        self.send(&id, DATA, &bytes)
      }
      Command::Fin { id, generation } if generation == self.generation => {
        self.send(&id, FIN, &[])?;
        if let Some(Link {
          stage: Stage::Active { local_fin, .. },
          ..
        }) = self.links.get_mut(&id)
        {
          *local_fin = true;
        }
        self.finish(&id);
        Ok(())
      }
      Command::Drained { id, generation } if generation == self.generation => {
        if let Some(Link {
          stage: Stage::Active { drained, .. },
          ..
        }) = self.links.get_mut(&id)
        {
          *drained = true;
        }
        self.finish(&id);
        Ok(())
      }
      Command::Credit { id, generation } if generation == self.generation => {
        if let Some(Link {
          stage: Stage::Active { received, .. },
          ..
        }) = self.links.get_mut(&id)
        {
          ensure!(*received > 0, "relay writer returned excess credit");
          *received -= 1;
          self.send(&id, CREDIT, &[])?;
        }
        Ok(())
      }
      Command::Abort { id, generation } if generation == self.generation => self.close(&id, true),
      _ => Ok(()),
    }
  }

  pub async fn shutdown(&mut self) {
    self.stop.cancel();
    for (id, link) in self.links.drain() {
      self.recent.insert(id);
      link.stop.cancel();
    }
    self.tasks.abort_all();
    while self.tasks.join_next().await.is_some() {}
  }
  pub fn recent(&self) -> Recent {
    self.recent.clone()
  }
}

impl Drop for Session {
  fn drop(&mut self) {
    self.stop.cancel();
  }
}
