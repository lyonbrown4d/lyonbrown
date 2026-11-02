use agent_memory::HttpEmbedding;
use memory_core::{EmbeddingDescriptor, MemoryEmbedding, MemoryError};
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;
use wiremock::{
  Mock, MockServer, ResponseTemplate,
  matchers::{body_json, header, method, path},
};
fn descriptor() -> EmbeddingDescriptor {
  EmbeddingDescriptor {
    id: "fixture".into(),
    model: "synthetic-model".into(),
    dimensions: 2,
    configuration_revision: "one".into(),
  }
}

#[tokio::test]
async fn local_protocol_adapter_sends_bounded_standard_embedding_request() -> anyhow::Result<()> {
  let server = MockServer::start().await;
  Mock::given(method("POST")).and(path("/v1/embeddings")).and(header("authorization","Bearer local-fixture-key"))
    .and(body_json(serde_json::json!({"model":"synthetic-model","input":"fixture text","encoding_format":"float"})))
    .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data":[{"index":0,"embedding":[0.25,0.75]}]}))).expect(1).mount(&server).await;
  let embedding = HttpEmbedding::new(
    &server.uri(),
    Some(SecretString::from("local-fixture-key")),
    descriptor(),
  )?;
  assert_eq!(
    embedding
      .embed("fixture text", CancellationToken::new())
      .await?,
    vec![0.25, 0.75]
  );
  Ok(())
}

#[tokio::test]
async fn malformed_vectors_oversized_bodies_and_redirects_fail_without_retries()
-> anyhow::Result<()> {
  let server = MockServer::start().await;
  Mock::given(path("/bad/embeddings"))
    .respond_with(
      ResponseTemplate::new(200)
        .set_body_json(serde_json::json!({"data":[{"index":0,"embedding":[1.0]}]})),
    )
    .expect(1)
    .mount(&server)
    .await;
  Mock::given(path("/large/embeddings"))
    .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(1_048_577)))
    .expect(1)
    .mount(&server)
    .await;
  Mock::given(path("/redirect/embeddings"))
    .respond_with(
      ResponseTemplate::new(302)
        .insert_header("Location", format!("{}/target/embeddings", server.uri())),
    )
    .expect(1)
    .mount(&server)
    .await;
  Mock::given(path("/target/embeddings"))
    .respond_with(ResponseTemplate::new(200))
    .expect(0)
    .mount(&server)
    .await;
  for suffix in ["bad", "large", "redirect"] {
    let adapter = HttpEmbedding::new(&format!("{}/{suffix}", server.uri()), None, descriptor())?;
    assert!(matches!(
      adapter.embed("fixture", CancellationToken::new()).await,
      Err(MemoryError::EmbeddingUnavailable)
    ));
  }
  assert!(matches!(
    HttpEmbedding::new("https://user:secret@example.invalid", None, descriptor()),
    Err(MemoryError::Invalid)
  ));
  Ok(())
}

#[tokio::test]
async fn cancelled_embeddings_issue_no_request_and_invalid_text_is_rejected() -> anyhow::Result<()>
{
  let server = MockServer::start().await;
  Mock::given(method("POST"))
    .respond_with(ResponseTemplate::new(500))
    .expect(0)
    .mount(&server)
    .await;
  let adapter = HttpEmbedding::new(&server.uri(), None, descriptor())?;
  let cancel = CancellationToken::new();
  cancel.cancel();
  assert!(matches!(
    adapter.embed("fixture", cancel).await,
    Err(MemoryError::Cancelled)
  ));
  assert!(matches!(
    adapter
      .embed(&"x".repeat(16_385), CancellationToken::new())
      .await,
    Err(MemoryError::Invalid)
  ));
  Ok(())
}
