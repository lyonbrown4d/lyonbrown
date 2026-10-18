use std::{collections::HashMap, pin::Pin, sync::Arc, time::Duration};

use agent_protocol::v1::RelayEnvelope;
use tokio::sync::{Mutex, mpsc, watch};
use tokio_stream::{Stream, StreamExt, wrappers::ReceiverStream};
use tonic::Status;

use crate::ControlPlane;

pub type RelayOutput = Pin<Box<dyn Stream<Item = Result<RelayEnvelope, Status>> + Send>>;
type RelaySender = mpsc::Sender<Result<RelayEnvelope, Status>>;
struct Connection {
  generation: uuid::Uuid,
  sender: RelaySender,
}

#[derive(Clone)]
pub(crate) struct RelayHub {
  connections: Arc<Mutex<HashMap<String, Connection>>>,
  queue: usize,
  shutdown: watch::Sender<bool>,
}
impl RelayHub {
  pub fn new(queue: usize) -> Self {
    Self {
      connections: Arc::default(),
      queue,
      shutdown: watch::channel(false).0,
    }
  }

  pub async fn attach(
    &self,
    control: ControlPlane,
    node: String,
    mut input: tonic::Streaming<RelayEnvelope>,
  ) -> Result<RelayOutput, Status> {
    if *self.shutdown.borrow() {
      return Err(Status::unavailable("control is shutting down"));
    }
    let generation = uuid::Uuid::now_v7();
    let (sender, receiver) = mpsc::channel(self.queue);
    {
      let mut connections = self.connections.lock().await;
      if connections
        .get(&node)
        .is_some_and(|connection| !connection.sender.is_closed())
      {
        return Err(Status::already_exists(
          "node already has an active outbound relay stream",
        ));
      }
      connections.insert(
        node.clone(),
        Connection {
          generation,
          sender: sender.clone(),
        },
      );
    }
    tracing::info!(operation = "control.relay", outcome = "attached", connection_id = %generation, node_id = %node, queue_capacity = self.queue, "authenticated outbound relay attached");
    let hub = self.clone();
    let output_control = control.clone();
    let mut shutdown = self.shutdown.subscribe();
    tokio::spawn(async move {
      let mut revoke = tokio::time::interval(Duration::from_millis(100));
      let failure = loop {
        if *shutdown.borrow() {
          break Some(Status::unavailable(
            "control is shutting down; reconnect without replay",
          ));
        }
        tokio::select! {
          _ = sender.closed() => break None,
          _ = shutdown.changed() => break Some(Status::unavailable("control is shutting down; reconnect without replay")),
          _ = revoke.tick() => {
            let authorized = control.peers.trust(&node).is_ok_and(|trust| trust.capabilities.iter().any(|cap| cap == "control.relay"));
            if !authorized { break Some(Status::permission_denied("relay identity was revoked")); }
          }
          message = input.message() => match message {
            Ok(Some(frame)) => if let Err(error) = hub.forward(&control, &node, frame).await { break Some(error); },
            Ok(None) => break None,
            Err(_) => break Some(Status::unavailable("outbound relay input disconnected; delivery result may be unknown")),
          }
        }
      };
      {
        let mut connections = hub.connections.lock().await;
        if connections
          .get(&node)
          .is_some_and(|connection| connection.generation == generation)
        {
          connections.remove(&node);
        }
      }
      if let Some(error) = failure {
        tracing::warn!(operation = "control.relay", outcome = "closed", connection_id = %generation, node_id = %node, code = ?error.code(), "relay stream closed; reconnect must not replay unconfirmed operations");
        // Awaiting space preserves explicit failure without silently discarding queued frames.
        let _ = sender.send(Err(error)).await;
      } else {
        tracing::info!(operation = "control.relay", outcome = "detached", connection_id = %generation, node_id = %node, "outbound relay disconnected");
      }
    });
    let verifier = ReceiverStream::new(receiver).map(move |item| {
      item.and_then(|frame| {
        output_control.allow(&frame.source_node_id, &frame.target_node_id)?;
        Ok(frame)
      })
    });
    Ok(Box::pin(verifier))
  }

  pub fn shutdown(&self) {
    self.shutdown.send_replace(true);
  }

  async fn forward(
    &self,
    control: &ControlPlane,
    node: &str,
    frame: RelayEnvelope,
  ) -> Result<(), Status> {
    if frame.source_node_id != node {
      return Err(Status::permission_denied(
        "relay source differs from client certificate identity",
      ));
    }
    if frame.link_id.is_empty()
      || frame.link_id.len() > 128
      || frame.target_node_id.is_empty()
      || frame.target_node_id.len() > 128
      || frame.ciphertext.is_empty()
      || frame.ciphertext.len() > 65_535
    {
      return Err(Status::invalid_argument(
        "invalid link, target identity or opaque ciphertext size",
      ));
    }
    control.allow(node, &frame.target_node_id)?;
    let sender = self
      .connections
      .lock()
      .await
      .get(&frame.target_node_id)
      .map(|connection| connection.sender.clone())
      .ok_or_else(|| Status::unavailable("target has no active outbound relay stream"))?;
    let source = frame.source_node_id.clone();
    let target = frame.target_node_id.clone();
    let mut shutdown = self.shutdown.subscribe();
    let mut recheck = tokio::time::interval(Duration::from_millis(100));
    let deadline = tokio::time::sleep(Duration::from_secs(2));
    let send = sender.send(Ok(frame));
    tokio::pin!(deadline, send);
    // Await bounded capacity without holding the directory mutex. Each source
    // stream has its own forwarder; endpoint multiplexers keep receiving while
    // their outbound queue is full, so symmetric traffic can continue draining.
    loop {
      if *shutdown.borrow() {
        return Err(Status::unavailable(
          "control is shutting down; delivery was not accepted",
        ));
      }
      tokio::select! {
        result = &mut send => return result.map_err(|_| Status::unavailable(
          "target relay stream disconnected; delivery was not accepted")),
        _ = &mut deadline => return Err(Status::resource_exhausted(
          "target relay remained backpressured for two seconds; delivery was not accepted")),
        _ = shutdown.changed() => return Err(Status::unavailable(
          "control is shutting down; delivery was not accepted")),
        _ = recheck.tick() => control.allow(&source, &target)?,
      }
    }
  }
}
