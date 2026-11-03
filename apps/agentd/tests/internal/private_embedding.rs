use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use clap::Parser;

struct Capture(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl MemoryEmbedding for Capture {
  fn descriptor(&self) -> EmbeddingDescriptor {
    EmbeddingDescriptor {
      id: "test".into(),
      model: "synthetic".into(),
      dimensions: 2,
      configuration_revision: "test".into(),
    }
  }
  async fn embed(&self, _: &str, _: CancellationToken) -> MemoryResult<Vec<f32>> {
    self.0.fetch_add(1, Ordering::SeqCst);
    Ok(vec![1.0, 0.0])
  }
}

#[tokio::test]
async fn embedding_refuses_static_and_newly_uploaded_secrets_before_backend_io()
-> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let secrets = Arc::new(SecretStore::open(directory.path())?);
  let calls = Arc::new(AtomicUsize::new(0));
  let mut config = crate::config::Config::try_parse_from(["agentd"])?;
  config.decision_api_key = Some("violet orchard decision credential".into());
  let engine = PrivateEmbedding::new(
    Arc::new(Capture(calls.clone())),
    &config,
    secrets.clone(),
    &[],
  )?;
  assert_eq!(
    engine
      .embed("ordinary project conventions", CancellationToken::new())
      .await?,
    vec![1.0, 0.0]
  );
  assert!(
    engine
      .embed(
        "violet orchard decision credential",
        CancellationToken::new()
      )
      .await
      .is_err()
  );
  secrets.put(
    "provider-fresh",
    &SecretString::from("copper meadow uploaded credential"),
  )?;
  assert!(
    engine
      .embed(
        "copper meadow uploaded credential",
        CancellationToken::new()
      )
      .await
      .is_err()
  );
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  Ok(())
}
