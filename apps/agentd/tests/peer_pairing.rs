use agent_distributed::identity::PeerIdentity;
use agentd::{
  Daemon,
  config::Config,
  peers::{Maintenance, PeerRuntime, maintain, manager},
};
use clap::Parser;

#[tokio::test]
async fn local_operator_must_verify_complete_tls_and_noise_binding_before_trust_changes() {
  let directory = tempfile::tempdir().unwrap();
  let mut config = Config::try_parse_from(["agentd"]).unwrap();
  config.data_dir = Some(directory.path().into());
  let daemon = Daemon::open(config).await.unwrap();
  assert!(
    PeerRuntime::prepare(daemon.clone())
      .await
      .unwrap()
      .is_none()
  );
  assert!(!directory.path().join("peers").exists());
  let remote = PeerIdentity::generate("explicit-remote").unwrap().public;
  let path = directory.path().join("remote-public.json");
  std::fs::write(&path, serde_json::to_vec(&remote).unwrap()).unwrap();
  let command = Maintenance::PeerPair {
    identity: path.clone(),
    fingerprint: remote.fingerprint().unwrap(),
    capabilities: vec!["tasks.submit".into()],
  };
  assert!(maintain(&daemon, &command).is_err());
  assert!(daemon.store.peers().unwrap().is_empty());
  let command = Maintenance::PeerPair {
    identity: path,
    fingerprint: remote.pairing_fingerprint().unwrap(),
    capabilities: vec!["tasks.submit".into()],
  };
  maintain(&daemon, &command).unwrap();
  let trusted = manager(&daemon).unwrap().trust(&remote.node_id).unwrap();
  assert_eq!(trusted.capabilities, vec!["tasks.submit"]);
  assert_eq!(trusted.noise_public_key, remote.noise_public_key);
  assert_eq!(trusted.certificate_pem, remote.certificate_pem);
  maintain(
    &daemon,
    &Maintenance::PeerRevoke {
      node_id: remote.node_id.clone(),
    },
  )
  .unwrap();
  assert!(manager(&daemon).unwrap().trust(&remote.node_id).is_err());
  assert!(daemon.store.peer(&remote.node_id).unwrap().unwrap().revoked);
}
