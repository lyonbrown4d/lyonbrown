use std::process::Command;

use agent_distributed::identity::PeerIdentity;
use agent_storage::Store;
use anyhow::Result;

#[cfg(unix)]
struct ChildGuard(std::process::Child);
#[cfg(unix)]
impl Drop for ChildGuard {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

#[test]
fn local_pairing_requires_human_verified_tls_and_noise_binding() -> Result<()> {
  let directory = tempfile::tempdir()?;
  let identity = PeerIdentity::generate("operator-paired-node")?;
  let bundle = directory.path().join("node-public.json");
  std::fs::write(&bundle, serde_json::to_vec(&identity.public)?)?;
  let data_dir = directory.path().join("control");
  let invoke = |fingerprint: &str| -> Result<std::process::Output> {
    Ok(
      Command::new(env!("CARGO_BIN_EXE_agent-control"))
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("pair")
        .arg(&bundle)
        .arg("--pairing-fingerprint")
        .arg(fingerprint)
        .output()?,
    )
  };
  let denied = invoke(&identity.public.fingerprint()?)?;
  assert!(
    !denied.status.success(),
    "certificate-only fingerprint must not authorize an independent Noise key"
  );
  let store = Store::open(data_dir.join("control.sqlite3"))?;
  assert!(store.peers()?.is_empty());
  let accepted = invoke(&identity.public.pairing_fingerprint()?)?;
  assert!(
    accepted.status.success(),
    "{}",
    String::from_utf8_lossy(&accepted.stderr)
  );
  let paired = store
    .peer("operator-paired-node")?
    .expect("verified peer persisted");
  assert_eq!(paired.noise_public_key, identity.public.noise_public_key);
  assert_eq!(paired.fingerprint, identity.public.fingerprint()?);
  assert!(!paired.revoked);
  let revoked = Command::new(env!("CARGO_BIN_EXE_agent-control"))
    .arg("--data-dir")
    .arg(&data_dir)
    .arg("revoke")
    .arg("operator-paired-node")
    .output()?;
  assert!(revoked.status.success());
  assert!(
    store
      .peer("operator-paired-node")?
      .expect("revocation persisted")
      .revoked
  );
  Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn executable_serves_real_mtls_and_drains_active_outbound_stream_on_interrupt() -> Result<()>
{
  use agent_distributed::relay_client::RelayClient;
  use std::{process::Stdio, time::Duration};
  let directory = tempfile::tempdir()?;
  let identity = PeerIdentity::generate("outbound-node")?;
  let bundle = directory.path().join("node.json");
  std::fs::write(&bundle, serde_json::to_vec(&identity.public)?)?;
  let paired = Command::new(env!("CARGO_BIN_EXE_agent-control"))
    .arg("--data-dir")
    .arg(directory.path())
    .arg("pair")
    .arg(&bundle)
    .arg("--pairing-fingerprint")
    .arg(identity.public.pairing_fingerprint()?)
    .output()?;
  assert!(
    paired.status.success(),
    "{}",
    String::from_utf8_lossy(&paired.stderr)
  );
  let control = PeerIdentity::load_or_create(&directory.path().join("identity"), "control")?;
  let reserved = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
  let address = reserved.local_addr()?;
  drop(reserved);
  let mut process = ChildGuard(
    Command::new(env!("CARGO_BIN_EXE_agent-control"))
      .arg("--data-dir")
      .arg(directory.path())
      .arg("--listen")
      .arg(address.to_string())
      .arg("serve")
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .spawn()?,
  );
  let endpoint = format!("https://{address}");
  let mut client = tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      match RelayClient::connect(&endpoint, &identity, &control.public).await {
        Ok(client) => break Ok::<_, anyhow::Error>(client),
        Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
      }
    }
  })
  .await??;
  let mut stream = client.open(1).await?;
  assert!(
    Command::new("kill")
      .arg("-INT")
      .arg(process.0.id().to_string())
      .status()?
      .success()
  );
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(3), stream.receive())
      .await?
      .expect_err("interrupt closes authenticated stream")
      .code(),
    tonic::Code::Unavailable
  );
  let status = tokio::time::timeout(Duration::from_secs(7), async {
    loop {
      if let Some(status) = process.0.try_wait()? {
        break Ok::<_, anyhow::Error>(status);
      }
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
  })
  .await??;
  assert!(status.success());
  Ok(())
}
