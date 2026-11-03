//! Embedding and vector SDKs are selected only at the daemon composition boundary.
use crate::config::Config;
use agent_config::MemoryVectorSettings;
use agent_memory::{MemoryService, RigEmbedding};
use fluxdi::{Error, Shared};
use memory_core::{EmbeddingDescriptor, MemoryRepository, VectorIndex};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub(super) async fn service(
  config: Shared<Config>,
  store: Shared<agent_storage::Store>,
) -> Result<Shared<MemoryService>, Error> {
  config
    .memory
    .validate()
    .map_err(|_| failure("invalid memory configuration"))?;
  let repository: Arc<dyn MemoryRepository> = store;
  let configured = config
    .memory
    .embedding
    .as_ref()
    .filter(|_| config.memory.enabled);
  let Some(embedding) = configured else {
    // No embedding configuration means no external calls, index creation, or
    // synthetic vectors. Authoritative memory CRUD remains available.
    return MemoryService::new(repository, None, None)
      .map(Shared::new)
      .map_err(|_| failure("memory service initialization failed"));
  };
  if embedding.api_key_env.is_some() && config.memory_embedding_api_key.is_none() {
    return Err(failure("embedding credential reference is unavailable"));
  }
  if matches!(
    &config.memory.vector,
    MemoryVectorSettings::Qdrant {
      api_key_env: Some(_),
      ..
    }
  ) && config.memory_vector_api_key.is_none()
  {
    return Err(failure("Qdrant credential reference is unavailable"));
  }
  let endpoint =
    url::Url::parse(&embedding.api_base).map_err(|_| failure("invalid embedding endpoint"))?;
  let identity = serde_json::to_vec(&(
    &embedding.id,
    &embedding.model,
    embedding.dimensions,
    endpoint.as_str(),
  ))
  .map_err(|_| failure("invalid embedding descriptor"))?;
  let descriptor = EmbeddingDescriptor {
    id: embedding.id.clone(),
    model: embedding.model.clone(),
    dimensions: embedding.dimensions,
    // Credentials rotate independently of embedding semantics.
    configuration_revision: hex::encode(Sha256::digest(identity)),
  };
  let engine = Arc::new(
    RigEmbedding::new(
      &embedding.api_base,
      config.memory_embedding_api_key.clone(),
      descriptor.clone(),
    )
    .map_err(|_| failure("embedding configuration is unavailable"))?,
  );
  let index: Arc<dyn VectorIndex> = match &config.memory.vector {
    MemoryVectorSettings::Embedded { directory } => {
      let data = config
        .data_path()
        .map_err(|_| failure("memory data directory unavailable"))?;
      let path = match directory {
        Some(path) if path.is_absolute() => path.clone(),
        Some(path) => data.join(path),
        None => data.join("memory").join(&descriptor.configuration_revision),
      };
      Arc::new(
        memory_qdrant::QdrantEmbeddedIndex::open(&path, descriptor)
          .await
          .map_err(|_| failure("embedded Qdrant index initialization failed"))?,
      )
    }
    MemoryVectorSettings::Qdrant {
      endpoint,
      collection,
      ..
    } => Arc::new(
      memory_qdrant::QdrantRemoteIndex::connect(
        endpoint,
        config.memory_vector_api_key.clone(),
        collection,
        descriptor,
        engine.model(),
      )
      .await
      .map_err(|_| failure("Qdrant server connection or compatibility check failed"))?,
    ),
  };
  MemoryService::new(repository, Some(engine), Some(index))
    .map(Shared::new)
    .map_err(|_| failure("memory service initialization failed"))
}

fn failure(message: &str) -> Error {
  Error::factory_failed("MemoryService", message)
}
