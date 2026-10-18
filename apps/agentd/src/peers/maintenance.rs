use std::path::PathBuf;

use agent_distributed::identity::PublicIdentity;
use anyhow::{Result, ensure};
use clap::Subcommand;

use crate::Daemon;

#[derive(Clone, Subcommand)]
pub enum Maintenance {
  /// Print this node's public peer identity and the full TLS + Noise pairing fingerprint.
  PeerIdentity,
  /// Import an explicitly verified peer. Stop/restart agentd to reload listener CA roots.
  PeerPair {
    #[arg(long)]
    identity: PathBuf,
    #[arg(long)]
    fingerprint: String,
    #[arg(long, value_delimiter = ',', required = true)]
    capabilities: Vec<String>,
  },
  /// Revoke a paired identity. Restart agentd to remove its listener CA root.
  PeerRevoke {
    #[arg(long)]
    node_id: String,
  },
}

pub fn maintain(daemon: &Daemon, command: &Maintenance) -> Result<()> {
  let manager = super::manager(daemon)?;
  match command {
    Maintenance::PeerIdentity => {
      let identity = manager.public_identity();
      println!("{}", serde_json::to_string(&identity)?);
      eprintln!("Pairing fingerprint: {}", identity.pairing_fingerprint()?);
    }
    Maintenance::PeerPair {
      identity,
      fingerprint,
      capabilities,
    } => {
      ensure!(
        std::fs::metadata(identity)?.len() <= 65_536,
        "public identity file exceeds limit"
      );
      let identity: PublicIdentity = serde_json::from_slice(&std::fs::read(identity)?)?;
      ensure!(
        identity.pairing_fingerprint()? == *fingerprint,
        "full pairing fingerprint differs; trust was not changed"
      );
      manager.pair_local(identity, capabilities.clone())?;
      println!("Peer paired; restart the peer listener to reload its trust roots.");
    }
    Maintenance::PeerRevoke { node_id } => {
      manager.revoke_local(node_id)?;
      println!("Peer revoked; restart the peer listener to reload its trust roots.");
    }
  }
  Ok(())
}
