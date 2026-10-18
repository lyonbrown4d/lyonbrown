use agent_distributed::{
  identity::PeerIdentity,
  secure_link::{LinkBinding, LinkHandshake},
};
use anyhow::Result;

#[test]
fn noise_roundtrip_rejects_replay_tampering_and_link_substitution() -> Result<()> {
  let first = PeerIdentity::generate("first")?;
  let second = PeerIdentity::generate("second")?;
  let binding = LinkBinding {
    link_id: "synthetic-link".into(),
    initiator: "first".into(),
    responder: "second".into(),
  };
  let mut initiator = LinkHandshake::new(&first, &second.public, binding.clone(), true)?;
  let mut responder = LinkHandshake::new(&second, &first.public, binding, false)?;
  responder.read(&initiator.write()?)?;
  initiator.read(&responder.write()?)?;
  let mut first = initiator.finish()?;
  let mut second = responder.finish()?;
  let frame = first.seal(b"synthetic private task data")?;
  assert!(
    !frame
      .ciphertext
      .windows(b"synthetic private task data".len())
      .any(|bytes| bytes == b"synthetic private task data")
  );
  assert_eq!(second.open(&frame)?, b"synthetic private task data");
  assert!(second.open(&frame).is_err());
  let mut tampered = second.seal(b"worker result")?;
  tampered.ciphertext[0] ^= 1;
  assert!(first.open(&tampered).is_err());
  let mut substituted = second.seal(b"next result")?;
  substituted.link_id = "different-link".into();
  assert!(first.open(&substituted).is_err());
  Ok(())
}

#[test]
fn responder_rejects_unpaired_static_key_after_handshake() -> Result<()> {
  let first = PeerIdentity::generate("first")?;
  let second = PeerIdentity::generate("second")?;
  let stranger = PeerIdentity::generate("stranger")?;
  let mut rotated_noise = first.public.clone();
  rotated_noise.noise_public_key = stranger.public.noise_public_key.clone();
  assert_eq!(rotated_noise.fingerprint()?, first.public.fingerprint()?);
  assert_ne!(
    rotated_noise.pairing_fingerprint()?,
    first.public.pairing_fingerprint()?
  );
  let mut wrong = stranger.public.clone();
  wrong.node_id = first.public.node_id.clone();
  let binding = LinkBinding {
    link_id: "test".into(),
    initiator: "first".into(),
    responder: "second".into(),
  };
  let mut initiator = LinkHandshake::new(&first, &second.public, binding.clone(), true)?;
  let mut responder = LinkHandshake::new(&second, &wrong, binding, false)?;
  responder.read(&initiator.write()?)?;
  initiator.read(&responder.write()?)?;
  assert!(responder.finish().is_err());
  Ok(())
}
