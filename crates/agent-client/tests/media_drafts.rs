use agent_client::media::ImageDraft;
const PNG: &[u8] = include_bytes!("../../agent-media/tests/fixtures/one.png");
#[test]
fn unconfirmed_submission_retains_exact_bytes_and_identity_until_matching_ack() -> anyhow::Result<()>
{
  let mut draft = ImageDraft::default();
  draft.add(
    "chosen.png".into(),
    agent_media::ImageAttachment::from_bytes(PNG.to_vec())?,
  )?;
  let first = draft.prepare(Some("session"), "describe", "request-a".into())?;
  let retry = draft.prepare(Some("session"), "describe", "request-b".into())?;
  assert_eq!(first.request_id, retry.request_id);
  assert_eq!(first.images, retry.images);
  assert!(draft.remove(0).is_err());
  assert!(
    draft
      .prepare(Some("different session"), "describe", "request-c".into())
      .is_err()
  );
  assert!(
    draft
      .prepare(Some("session"), "changed prompt", "request-c".into())
      .is_err()
  );
  assert!(!draft.acknowledge("unrelated"));
  assert_eq!(draft.attachments.len(), 1);
  assert!(draft.acknowledge("request-a"));
  assert!(draft.attachments.is_empty());
  assert!(draft.pending.is_none());
  Ok(())
}

#[test]
fn definite_rejection_unlocks_editing_while_preserving_selected_images() -> anyhow::Result<()> {
  let mut draft = ImageDraft::default();
  draft.add(
    "chosen.png".into(),
    agent_media::ImageAttachment::from_bytes(PNG.to_vec())?,
  )?;
  draft.prepare(Some("session"), "describe", "request".into())?;
  draft.reject("unrelated");
  assert!(draft.pending.is_some());
  draft.reject("request");
  assert!(draft.pending.is_none());
  assert_eq!(draft.attachments.len(), 1);
  draft.remove(0)?;
  assert!(draft.attachments.is_empty());
  Ok(())
}
