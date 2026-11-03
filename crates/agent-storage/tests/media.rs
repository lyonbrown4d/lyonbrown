use agent_core::new_id;
use agent_media::ImageAttachment;
use agent_storage::Store;
use sea_orm::ConnectionTrait;
const PNG: &[u8] = include_bytes!("../../agent-media/tests/fixtures/one.png");
#[tokio::test]
async fn media_and_run_are_atomic_private_and_idempotent() -> anyhow::Result<()> {
  let store = Store::open(":memory:").await?;
  let session = store.create_session("images", "/fixture", "node").await?;
  let images = vec![ImageAttachment::from_bytes(PNG.to_vec())?];
  let request = new_id();
  let before = store.pending_outbox(100).await?;
  store.connection().execute_unprepared("CREATE TRIGGER reject_media_event BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'injected fault'); END;").await?;
  assert!(
    store
      .accept_run_with_media(&session.id, 1, "", &request, &images)
      .await
      .is_err()
  );
  assert!(store.runs(&session.id).await?.is_empty());
  assert_eq!(store.pending_outbox(100).await?, before);
  store
    .connection()
    .execute_unprepared("DROP TRIGGER reject_media_event;")
    .await?;
  let (run, created) = store
    .accept_run_with_media(&session.id, 1, "", &request, &images)
    .await?;
  assert!(created);
  assert_eq!(
    store
      .accepted_run_with_media(&session.id, 1, "", &request, &images)
      .await?
      .expect("ack")
      .id,
    run.id
  );
  assert!(
    store
      .accepted_run_with_media(&session.id, 1, "", &new_id(), &images)
      .await?
      .is_none()
  );
  assert!(run.prompt.is_empty());
  assert_eq!(store.run_media(&run.id).await?, images);
  let (same, created) = store
    .accept_run_with_media(&session.id, 1, "", &request, &images)
    .await?;
  assert!(!created);
  assert_eq!(same, run);
  assert!(
    store
      .accept_run(&session.id, 1, "", &request)
      .await
      .is_err()
  );
  let mut changed = images.clone();
  changed[0].sha256 = "0".repeat(64);
  assert!(
    store
      .accept_run_with_media(&session.id, 1, "", &request, &changed)
      .await
      .is_err()
  );
  assert!(
    store
      .accept_run_with_media(&session.id, 2, "", &new_id(), &images)
      .await
      .is_err()
  );
  let events = store.events(&run.id, 0, 100).await?;
  let payloads = events
    .iter()
    .map(|event| event.payload_json.as_str())
    .collect::<Vec<_>>()
    .join("");
  assert!(!payloads.contains("data"));
  assert!(!payloads.contains(&images[0].sha256));
  assert_eq!(store.pending_outbox(100).await?, events);
  Ok(())
}
