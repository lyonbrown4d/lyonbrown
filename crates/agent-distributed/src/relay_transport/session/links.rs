use anyhow::{Result, ensure};
use tokio::{sync::oneshot, time::Instant};

use super::super::{CONNECT, RelayIo, bridge};
use super::{ABORT, DATA, HANDSHAKE, Link, MAX_LINKS, MAX_PENDING, Session, Stage, VERSION};
use crate::{
  peers::public_identity,
  secure_link::{LinkBinding, LinkHandshake, SecureLink},
};

impl Session {
  pub(super) fn paired(&self, peer: &str) -> Result<String> {
    public_identity(&self.manager.trust(peer)?).pairing_fingerprint()
  }

  pub(super) fn open(
    &mut self,
    peer: String,
    reply: oneshot::Sender<Result<RelayIo>>,
  ) -> Result<()> {
    let setup = (|| {
      ensure!(
        self.links.len() < MAX_LINKS && self.pending() < MAX_PENDING,
        "relay link or pending handshake limit reached"
      );
      let paired = public_identity(&self.manager.trust(&peer)?);
      let fingerprint = paired.pairing_fingerprint()?;
      let id = uuid::Uuid::now_v7().to_string();
      let binding = LinkBinding {
        link_id: id.clone(),
        initiator: self.manager.node.id.clone(),
        responder: peer.clone(),
      };
      let mut handshake = LinkHandshake::new(&self.manager.identity, &paired, binding, true)?;
      let hello = self.handshake_frame(&id, &peer, handshake.write()?);
      Ok::<_, anyhow::Error>((id, fingerprint, handshake, hello))
    })();
    let (id, fingerprint, handshake, hello) = match setup {
      Ok(setup) => setup,
      Err(error) => {
        let _ = reply.send(Err(error));
        return Ok(());
      }
    };
    // Queue failure closes this entire generation; skipping a frame is never permitted.
    self.queue(hello)?;
    self.links.insert(
      id.clone(),
      Link {
        id,
        peer,
        fingerprint,
        activity: Instant::now(),
        stop: self.stop.child_token(),
        stage: Stage::Pending {
          handshake: Some(Box::new(handshake)),
          reply: Some(reply),
        },
      },
    );
    Ok(())
  }

  pub(super) fn active(
    &mut self,
    id: &str,
    secure: SecureLink,
    stop: tokio_util::sync::CancellationToken,
  ) -> (Stage, RelayIo) {
    let (io, incoming, credit) = bridge::start(
      id.into(),
      self.generation,
      self.commands.clone(),
      stop,
      &mut self.tasks,
    );
    (
      Stage::Active {
        secure: Box::new(secure),
        incoming: Some(incoming),
        credit,
        outstanding: 0,
        received: 0,
        local_fin: false,
        remote_fin: false,
        drained: false,
      },
      io,
    )
  }

  pub(super) fn send(&mut self, id: &str, tag: u8, payload: &[u8]) -> Result<()> {
    let Some(link) = self.links.get(id) else {
      return Ok(());
    };
    if !self
      .paired(&link.peer)
      .is_ok_and(|fingerprint| fingerprint == link.fingerprint)
    {
      return self.close(id, false);
    }
    let Some(link) = self.links.get_mut(id) else {
      return Ok(());
    };
    if let Stage::Active {
      secure,
      local_fin,
      outstanding,
      ..
    } = &mut link.stage
    {
      if tag == DATA {
        ensure!(!*local_fin, "relay data write after EOF");
        ensure!(
          *outstanding < bridge::WINDOW,
          "relay outbound credit exceeded"
        );
        *outstanding += 1;
      }
      let mut bytes = Vec::with_capacity(payload.len() + 2);
      bytes.extend_from_slice(&[VERSION, tag]);
      bytes.extend_from_slice(payload);
      let frame = secure.seal(&bytes)?;
      link.activity = Instant::now();
      self.queue(frame)?;
    }
    Ok(())
  }

  pub(super) fn close(&mut self, id: &str, notify: bool) -> Result<()> {
    if notify && self.outgoing.capacity() > 0 {
      self.send(id, ABORT, &[])?;
    }
    if let Some(link) = self.links.remove(id) {
      link.stop.cancel();
      self.recent.insert(id.into());
    }
    Ok(())
  }

  pub(super) fn finish(&mut self, id: &str) {
    if self.links.get(id).is_some_and(|link| {
      matches!(
        link.stage,
        Stage::Active {
          local_fin: true,
          remote_fin: true,
          drained: true,
          ..
        }
      )
    }) {
      // Both pumps finished normally, preserving queued bytes before EOF.
      self.links.remove(id);
      self.recent.insert(id.into());
    }
  }

  pub(super) fn expire(&mut self) -> Result<()> {
    let mut expired = Vec::new();
    for (id, link) in &self.links {
      let revoked = !self
        .paired(&link.peer)
        .is_ok_and(|value| value == link.fingerprint);
      let timeout = if matches!(link.stage, Stage::Pending { .. }) {
        CONNECT
      } else {
        std::time::Duration::from_secs(30)
      };
      if revoked || link.activity.elapsed() >= timeout {
        expired.push((id.clone(), !revoked));
      }
    }
    for (id, notify) in expired {
      self.close(&id, notify)?;
    }
    Ok(())
  }

  pub(super) fn pending(&self) -> usize {
    self
      .links
      .values()
      .filter(|link| matches!(link.stage, Stage::Pending { .. }))
      .count()
  }

  pub(super) fn handshake_frame(
    &self,
    id: &str,
    peer: &str,
    ciphertext: Vec<u8>,
  ) -> agent_protocol::v1::RelayEnvelope {
    agent_protocol::v1::RelayEnvelope {
      link_id: id.into(),
      source_node_id: self.manager.node.id.clone(),
      target_node_id: peer.into(),
      seq: HANDSHAKE,
      ciphertext,
    }
  }
}
