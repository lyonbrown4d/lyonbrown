use memory_core::{
  EmbeddingDescriptor, IndexError, IndexedMemory, MemoryScope, VectorIndex, VectorQuery,
};
use memory_qdrant::QdrantEmbeddedIndex;
use tokio_util::sync::CancellationToken;

fn descriptor() -> EmbeddingDescriptor {
  EmbeddingDescriptor {
    id: "fixture".into(),
    model: "synthetic".into(),
    dimensions: 3,
    configuration_revision: "v1".into(),
  }
}
fn point(id: u128, revision: u64, scope: MemoryScope, vector: Vec<f32>) -> IndexedMemory {
  IndexedMemory {
    id: uuid::Uuid::from_u128(id).to_string(),
    revision,
    scope,
    embedding: descriptor(),
    vector,
  }
}
fn query(scopes: Vec<MemoryScope>) -> VectorQuery {
  VectorQuery {
    vector: vec![1., 0., 0.],
    scopes,
    embedding: descriptor(),
    limit: 10,
  }
}
fn cancel() -> CancellationToken {
  CancellationToken::new()
}

#[tokio::test]
async fn embedded_scopes_revisions_durability_and_logical_delete()
-> Result<(), Box<dyn std::error::Error>> {
  let directory = tempfile::tempdir()?;
  let index = QdrantEmbeddedIndex::open(directory.path(), descriptor()).await?;
  assert!(
    QdrantEmbeddedIndex::open(directory.path(), descriptor())
      .await
      .is_err(),
    "single native owner"
  );
  let project = MemoryScope::Project("same-id".into());
  let user = MemoryScope::User("same-id".into());
  let newest = point(1, 2, project.clone(), vec![1., 0., 0.]);
  let older = point(1, 1, project.clone(), vec![0., 1., 0.]);
  let foreign = point(2, 1, user.clone(), vec![1., 0., 0.]);
  index
    .upsert(vec![newest.clone(), foreign], cancel())
    .await?;
  index.upsert(vec![older.clone()], cancel()).await?;
  index.upsert(vec![older], cancel()).await?;
  let hits = index.search(query(vec![project.clone()]), cancel()).await?;
  assert_eq!(hits.len(), 2, "same revision retry is idempotent");
  assert_eq!(hits[0].id, newest.id);
  assert_eq!(
    hits[0].revision, 2,
    "late old work cannot overwrite revision 2"
  );
  let user_hits = index.search(query(vec![user.clone()]), cancel()).await?;
  assert_eq!(user_hits.len(), 1);
  assert_eq!(user_hits[0].id, uuid::Uuid::from_u128(2).to_string());
  index.maintain(cancel()).await?;
  assert!(index.status().await?.ready);
  drop(index);
  // Drop flushes off the Tokio worker; wait for the native owner to release.
  let reopened = tokio::time::timeout(std::time::Duration::from_secs(10), async {
    loop {
      if let Ok(index) = QdrantEmbeddedIndex::open(directory.path(), descriptor()).await {
        break index;
      }
      tokio::task::yield_now().await;
    }
  })
  .await?;
  assert_eq!(
    reopened
      .search(query(vec![project.clone()]), cancel())
      .await?
      .len(),
    2
  );
  reopened.delete(&newest.id, 1, cancel()).await?;
  let retained = reopened
    .search(query(vec![project.clone()]), cancel())
    .await?;
  assert_eq!(retained.len(), 1);
  assert_eq!(
    retained[0].revision, 2,
    "late deletion of revision 1 retains newer active memory"
  );
  reopened.delete(&newest.id, 3, cancel()).await?;
  assert!(
    reopened
      .search(query(vec![project]), cancel())
      .await?
      .is_empty()
  );
  assert_eq!(reopened.search(query(vec![user]), cancel()).await?.len(), 1);
  reopened.maintain(cancel()).await?;
  Ok(())
}

#[tokio::test]
async fn embedded_rejects_invalid_vectors_scopes_and_pre_cancelled_writes()
-> Result<(), Box<dyn std::error::Error>> {
  let directory = tempfile::tempdir()?;
  let index = QdrantEmbeddedIndex::open(directory.path(), descriptor()).await?;
  let scope = MemoryScope::Session("session".into());
  let bad = point(1, 1, scope.clone(), vec![1., 0.]);
  assert!(matches!(
    index.upsert(vec![bad], cancel()).await,
    Err(IndexError::Invalid)
  ));
  let bad = point(1, 1, scope.clone(), vec![f32::NAN, 0., 1.]);
  assert!(matches!(
    index.upsert(vec![bad], cancel()).await,
    Err(IndexError::Invalid)
  ));
  assert!(matches!(
    index.search(query(vec![]), cancel()).await,
    Err(IndexError::Invalid)
  ));
  let mut wrong_descriptor = query(vec![scope.clone()]);
  wrong_descriptor.embedding.configuration_revision = "v2".into();
  assert!(matches!(
    index.search(wrong_descriptor, cancel()).await,
    Err(IndexError::Invalid)
  ));
  let token = cancel();
  token.cancel();
  assert!(matches!(
    index
      .upsert(vec![point(1, 1, scope.clone(), vec![1., 0., 0.])], token)
      .await,
    Err(IndexError::Cancelled)
  ));
  assert!(index.search(query(vec![scope]), cancel()).await?.is_empty());
  Ok(())
}

#[tokio::test]
async fn revision_cutoff_remains_exact_above_f64_integer_precision()
-> Result<(), Box<dyn std::error::Error>> {
  let directory = tempfile::tempdir()?;
  let index = QdrantEmbeddedIndex::open(directory.path(), descriptor()).await?;
  let scope = MemoryScope::User("precision".into());
  // Both IEEE rounding directions and the high/low component boundary.
  for cutoff in [(1_u64 << 32) - 1, (1_u64 << 53) + 1, (1_u64 << 53) + 3] {
    index
      .upsert(
        vec![
          point(1, cutoff, scope.clone(), vec![1., 0., 0.]),
          point(1, cutoff + 1, scope.clone(), vec![1., 0., 0.]),
        ],
        cancel(),
      )
      .await?;
    index
      .delete(&uuid::Uuid::from_u128(1).to_string(), cutoff, cancel())
      .await?;
    let retained = index.search(query(vec![scope.clone()]), cancel()).await?;
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].revision, cutoff + 1);
    index
      .delete(
        &uuid::Uuid::from_u128(1).to_string(),
        i64::MAX as u64,
        cancel(),
      )
      .await?;
    assert!(
      index
        .search(query(vec![scope.clone()]), cancel())
        .await?
        .is_empty()
    );
  }
  Ok(())
}
