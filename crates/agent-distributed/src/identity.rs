use std::{
  fs::{self, OpenOptions},
  io::Write,
  path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tonic::transport::{Certificate, ClientTlsConfig, Identity, ServerTlsConfig};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicIdentity {
  pub node_id: String,
  pub certificate_pem: String,
  pub noise_public_key: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PeerIdentity {
  pub public: PublicIdentity,
  private_key_pem: String,
  noise_private_key: String,
}

impl PeerIdentity {
  pub fn load_or_create(directory: &Path, node_id: &str) -> Result<Self> {
    fs::create_dir_all(directory)?;
    let path = directory.join("peer-identity.json");
    if path.exists() {
      let identity: Self =
        serde_json::from_slice(&fs::read(&path)?).context("invalid peer identity file")?;
      ensure!(
        identity.public.node_id == node_id,
        "peer identity belongs to another node"
      );
      identity.public.validate()?;
      return Ok(identity);
    }
    let identity = Self::generate(node_id)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
      fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
      options.mode(0o600);
    }
    let mut file = options.open(path).context("create private peer identity")?;
    file.write_all(&serde_json::to_vec(&identity)?)?;
    file.sync_all()?;
    Ok(identity)
  }

  pub fn generate(node_id: &str) -> Result<Self> {
    ensure!(
      !node_id.is_empty() && node_id.len() <= 128,
      "invalid node identity"
    );
    let generated = rcgen::generate_simple_self_signed(vec![server_name(node_id)])?;
    let noise =
      snow::Builder::new("Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?).generate_keypair()?;
    Ok(Self {
      public: PublicIdentity {
        node_id: node_id.into(),
        certificate_pem: generated.cert.pem(),
        noise_public_key: hex::encode(noise.public),
      },
      private_key_pem: generated.signing_key.serialize_pem(),
      noise_private_key: hex::encode(noise.private),
    })
  }

  pub fn client_tls(&self, remote: &PublicIdentity) -> Result<ClientTlsConfig> {
    agent_security::initialize_tls();
    remote.validate()?;
    Ok(
      ClientTlsConfig::new()
        .domain_name(server_name(&remote.node_id))
        .ca_certificate(Certificate::from_pem(&remote.certificate_pem))
        .identity(self.tls_identity()),
    )
  }

  pub fn server_tls(
    &self,
    peers: impl IntoIterator<Item = PublicIdentity>,
  ) -> Result<ServerTlsConfig> {
    agent_security::initialize_tls();
    let mut certificates = String::new();
    for peer in peers {
      peer.validate()?;
      certificates.push_str(&peer.certificate_pem);
      certificates.push('\n');
    }
    ensure!(
      !certificates.is_empty(),
      "pair at least one peer before enabling the mTLS listener"
    );
    Ok(
      ServerTlsConfig::new()
        .identity(self.tls_identity())
        .client_ca_root(Certificate::from_pem(certificates)),
    )
  }

  pub(crate) fn noise_private(&self) -> Result<Vec<u8>> {
    Ok(hex::decode(&self.noise_private_key)?)
  }

  fn tls_identity(&self) -> Identity {
    Identity::from_pem(&self.public.certificate_pem, &self.private_key_pem)
  }
}

impl PublicIdentity {
  pub fn fingerprint(&self) -> Result<String> {
    certificate_fingerprint(self.certificate_pem.as_bytes())
  }
  /// Human pairing must verify this complete binding when using an untrusted directory.
  /// The certificate-only fingerprint identifies mTLS, not the independent Noise key.
  pub fn pairing_fingerprint(&self) -> Result<String> {
    let binding = serde_json::to_vec(&(
      "lyonbrown-pair-v1",
      &self.node_id,
      self.fingerprint()?,
      &self.noise_public_key,
    ))?;
    Ok(hex::encode(Sha256::digest(binding)))
  }
  pub fn validate(&self) -> Result<()> {
    ensure!(
      !self.node_id.is_empty() && self.node_id.len() <= 128,
      "invalid peer node identity"
    );
    self.fingerprint()?;
    ensure!(
      hex::decode(&self.noise_public_key)?.len() == 32,
      "invalid peer Noise public key"
    );
    Ok(())
  }
}

pub fn certificate_fingerprint(pem: &[u8]) -> Result<String> {
  let certificates =
    rustls_pemfile::certs(&mut std::io::Cursor::new(pem)).collect::<std::io::Result<Vec<_>>>()?;
  ensure!(
    certificates.len() == 1,
    "peer identity must contain exactly one certificate"
  );
  Ok(hex::encode(Sha256::digest(certificates[0].as_ref())))
}

fn server_name(node_id: &str) -> String {
  let digest = hex::encode(Sha256::digest(node_id.as_bytes()));
  format!("peer-{}.lyonbrown.invalid", &digest[..24])
}
