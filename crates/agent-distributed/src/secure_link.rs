use agent_protocol::v1::RelayEnvelope;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::identity::{PeerIdentity, PublicIdentity};

const MAX_FRAME: usize = 65_535;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkBinding {
  pub link_id: String,
  pub initiator: String,
  pub responder: String,
}

/// Noise IK authenticates pre-paired static keys; the relay never receives the keys.
pub struct LinkHandshake {
  state: snow::HandshakeState,
  binding: LinkBinding,
  local: String,
  remote: String,
  expected_remote_key: Vec<u8>,
}

impl LinkHandshake {
  pub fn new(
    identity: &PeerIdentity,
    peer: &PublicIdentity,
    binding: LinkBinding,
    initiator: bool,
  ) -> Result<Self> {
    peer.validate()?;
    ensure!(
      !binding.link_id.is_empty() && binding.link_id.len() <= 128,
      "invalid relay link identity"
    );
    let (local, remote) = if initiator {
      (&binding.initiator, &binding.responder)
    } else {
      (&binding.responder, &binding.initiator)
    };
    ensure!(
      local == &identity.public.node_id && remote == &peer.node_id && local != remote,
      "relay link identities do not match paired peers"
    );
    let private = identity.noise_private()?;
    let expected_remote_key = hex::decode(&peer.noise_public_key)?;
    let prologue = serde_json::to_vec(&("lyonbrown-relay-v1", &binding))?;
    let builder = snow::Builder::new("Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?)
      .local_private_key(&private)?
      .prologue(&prologue)?;
    let state = if initiator {
      builder
        .remote_public_key(&expected_remote_key)?
        .build_initiator()?
    } else {
      builder.build_responder()?
    };
    Ok(Self {
      state,
      local: local.clone(),
      remote: remote.clone(),
      binding,
      expected_remote_key,
    })
  }

  pub fn write(&mut self) -> Result<Vec<u8>> {
    let mut message = vec![0; MAX_FRAME];
    let size = self.state.write_message(&[], &mut message)?;
    message.truncate(size);
    Ok(message)
  }
  pub fn read(&mut self, message: &[u8]) -> Result<()> {
    ensure!(
      message.len() <= MAX_FRAME,
      "relay handshake frame exceeds limits"
    );
    let mut output = vec![0; MAX_FRAME];
    ensure!(
      self.state.read_message(message, &mut output)? == 0,
      "application payload is forbidden before peer authentication"
    );
    Ok(())
  }
  pub fn finish(self) -> Result<SecureLink> {
    ensure!(
      self.state.is_handshake_finished(),
      "relay peer handshake is incomplete"
    );
    ensure!(
      self.state.get_remote_static() == Some(self.expected_remote_key.as_slice()),
      "relay static identity does not match paired peer"
    );
    Ok(SecureLink {
      state: self.state.into_transport_mode()?,
      binding: self.binding,
      local: self.local,
      remote: self.remote,
      send_sequence: 0,
      receive_sequence: 0,
    })
  }
}

pub struct SecureLink {
  state: snow::TransportState,
  binding: LinkBinding,
  local: String,
  remote: String,
  send_sequence: u64,
  receive_sequence: u64,
}

impl SecureLink {
  pub fn seal(&mut self, plaintext: &[u8]) -> Result<RelayEnvelope> {
    ensure!(
      plaintext.len() <= MAX_FRAME - 16,
      "relay plaintext exceeds frame limits"
    );
    let next = self
      .send_sequence
      .checked_add(1)
      .ok_or_else(|| anyhow::anyhow!("relay send sequence exhausted"))?;
    let mut ciphertext = vec![0; plaintext.len() + 16];
    let size = self.state.write_message(plaintext, &mut ciphertext)?;
    ciphertext.truncate(size);
    let frame = RelayEnvelope {
      link_id: self.binding.link_id.clone(),
      source_node_id: self.local.clone(),
      target_node_id: self.remote.clone(),
      seq: self.send_sequence,
      ciphertext,
    };
    self.send_sequence = next;
    Ok(frame)
  }
  pub fn open(&mut self, frame: &RelayEnvelope) -> Result<Vec<u8>> {
    ensure!(
      frame.link_id == self.binding.link_id
        && frame.source_node_id == self.remote
        && frame.target_node_id == self.local,
      "relay frame is bound to a different authenticated link"
    );
    ensure!(
      frame.seq == self.receive_sequence,
      "relay frame was replayed or arrived out of order"
    );
    ensure!(
      frame.ciphertext.len() >= 16 && frame.ciphertext.len() <= MAX_FRAME,
      "invalid relay ciphertext size"
    );
    let next = self
      .receive_sequence
      .checked_add(1)
      .ok_or_else(|| anyhow::anyhow!("relay receive sequence exhausted"))?;
    let mut output = vec![0; frame.ciphertext.len()];
    let size = self.state.read_message(&frame.ciphertext, &mut output)?;
    output.truncate(size);
    self.receive_sequence = next;
    Ok(output)
  }
}
