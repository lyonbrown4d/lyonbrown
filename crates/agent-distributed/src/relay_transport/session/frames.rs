use agent_protocol::v1::RelayEnvelope;
use anyhow::{Result, ensure};
use tokio::time::Instant;

use super::{
  ABORT, CREDIT, DATA, FIN, HANDSHAKE, Link, MAX_LINKS, MAX_PENDING, Session, Stage, VERSION,
};
use crate::{
  peers::public_identity,
  secure_link::{LinkBinding, LinkHandshake},
};

impl Session {
  pub(super) fn frame(&mut self, frame: RelayEnvelope) -> Result<()> {
    if frame.target_node_id != self.manager.node.id
      || frame.source_node_id == self.manager.node.id
      || frame.ciphertext.len() > 65_535
      || frame.ciphertext.is_empty()
      || uuid::Uuid::parse_str(&frame.link_id).is_err()
    {
      return Ok(());
    }
    let Some(mut link) = self.links.remove(&frame.link_id) else {
      return self.initiation(frame);
    };
    if frame.source_node_id != link.peer
      || !self
        .paired(&link.peer)
        .is_ok_and(|fingerprint| fingerprint == link.fingerprint)
    {
      link.stop.cancel();
      self.recent.insert(link.id);
      return Ok(());
    }
    let handled = self.known(&mut link, &frame);
    let id = link.id.clone();
    self.links.insert(id.clone(), link);
    if let Err(error) = handled {
      tracing::debug!(operation = "relay.link", outcome = "closed", link_id = %id,
        reason = %error, "authenticated relay link closed");
      self.close(&id, true)?;
    } else {
      self.finish(&id);
    }
    Ok(())
  }

  fn known(&mut self, link: &mut Link, frame: &RelayEnvelope) -> Result<()> {
    match &mut link.stage {
      Stage::Pending { handshake, reply } => {
        ensure!(
          frame.seq == HANDSHAKE,
          "data arrived before Noise authentication"
        );
        let mut handshake = handshake
          .take()
          .ok_or_else(|| anyhow::anyhow!("relay handshake missing"))?;
        handshake.read(&frame.ciphertext)?;
        let secure = (*handshake).finish()?;
        let reply = reply
          .take()
          .ok_or_else(|| anyhow::anyhow!("relay open result missing"))?;
        let (stage, io) = self.active(&link.id, secure, link.stop.clone());
        link.stage = stage;
        ensure!(reply.send(Ok(io)).is_ok(), "relay open caller cancelled");
      }
      Stage::Active {
        secure,
        incoming,
        credit,
        outstanding,
        received,
        remote_fin,
        ..
      } => {
        ensure!(frame.seq != HANDSHAKE, "duplicate relay handshake");
        let bytes = secure.open(frame)?;
        ensure!(
          bytes.len() >= 2 && bytes[0] == VERSION,
          "invalid relay byte-stream version"
        );
        match bytes[1] {
          DATA => {
            ensure!(
              !*remote_fin
                && bytes.len() <= super::super::bridge::CHUNK + 2
                && *received < super::super::bridge::WINDOW,
              "invalid relay data after EOF or chunk limit"
            );
            incoming
              .as_ref()
              .ok_or_else(|| anyhow::anyhow!("relay reader closed"))?
              .try_send(bytes[2..].to_vec())
              .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                  anyhow::anyhow!("relay per-link receive credit invariant violated")
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                  anyhow::anyhow!("relay local byte reader closed")
                }
              })?;
            *received += 1;
          }
          FIN => {
            ensure!(bytes.len() == 2 && !*remote_fin, "invalid relay EOF");
            *remote_fin = true;
            incoming.take();
          }
          ABORT => anyhow::bail!("remote closed relay link"),
          CREDIT => {
            ensure!(
              bytes.len() == 2 && *outstanding > 0,
              "relay peer returned excess credit"
            );
            *outstanding -= 1;
            credit.add_permits(1);
          }
          _ => anyhow::bail!("unknown relay byte-stream tag"),
        }
      }
    }
    link.activity = Instant::now();
    Ok(())
  }

  fn initiation(&mut self, frame: RelayEnvelope) -> Result<()> {
    if frame.seq != HANDSHAKE
      || self.recent.ids.contains(&frame.link_id)
      || self.links.len() >= MAX_LINKS
      || self.pending() >= MAX_PENDING
      || self.outgoing.capacity() == 0
    {
      return Ok(());
    }
    let authenticated = (|| {
      let paired = public_identity(&self.manager.trust(&frame.source_node_id)?);
      let fingerprint = paired.pairing_fingerprint()?;
      let binding = LinkBinding {
        link_id: frame.link_id.clone(),
        initiator: frame.source_node_id.clone(),
        responder: self.manager.node.id.clone(),
      };
      let mut handshake = LinkHandshake::new(&self.manager.identity, &paired, binding, false)?;
      handshake.read(&frame.ciphertext)?;
      Ok::<_, anyhow::Error>((fingerprint, handshake.write()?, handshake.finish()?))
    })();
    let Ok((fingerprint, response, secure)) = authenticated else {
      return Ok(());
    };
    let stop = self.stop.child_token();
    let (stage, io) = self.active(&frame.link_id, secure, stop.clone());
    if self.incoming.try_send(Ok(io)).is_err() {
      stop.cancel();
      self.recent.insert(frame.link_id);
      return Ok(());
    }
    self.queue(self.handshake_frame(&frame.link_id, &frame.source_node_id, response))?;
    self.links.insert(
      frame.link_id.clone(),
      Link {
        id: frame.link_id,
        peer: frame.source_node_id,
        fingerprint,
        activity: Instant::now(),
        stop,
        stage,
      },
    );
    Ok(())
  }
}
