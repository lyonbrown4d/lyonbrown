//! The descriptor is published atomically before any shard data is created.
use crate::payload::VECTOR;
use fs2::FileExt;
use memory_core::{EmbeddingDescriptor, IndexError, IndexResult};
use qdrant_edge::*;
use std::{
  fs::File,
  io::{Read, Write},
  path::Path,
};

pub(crate) fn open(path: &Path, expected: &EmbeddingDescriptor) -> IndexResult<(EdgeShard, File)> {
  std::fs::create_dir_all(path).map_err(|_| IndexError::Unavailable)?;
  let lock = std::fs::OpenOptions::new()
    .create(true)
    .truncate(false)
    .read(true)
    .write(true)
    .open(path.join("owner.lock"))
    .map_err(|_| IndexError::Unavailable)?;
  lock
    .try_lock_exclusive()
    .map_err(|_| IndexError::Unavailable)?;
  let metadata = path.join("embedding.json");
  let shard_path = path.join("shard");
  if metadata.try_exists().map_err(|_| IndexError::Unavailable)? {
    let mut bytes = Vec::new();
    File::open(&metadata)
      .map_err(|_| IndexError::Unavailable)?
      .take(4097)
      .read_to_end(&mut bytes)
      .map_err(|_| IndexError::Unavailable)?;
    if bytes.len() > 4096 {
      return Err(IndexError::Invalid);
    }
    let persisted: EmbeddingDescriptor =
      serde_json::from_slice(&bytes).map_err(|_| IndexError::Unavailable)?;
    if persisted != *expected {
      return Err(IndexError::Invalid);
    }
  } else {
    // Never adopt a preexisting shard whose embedding identity is unknown.
    if shard_path
      .try_exists()
      .map_err(|_| IndexError::Unavailable)?
    {
      return Err(IndexError::Unavailable);
    }
    let bytes = serde_json::to_vec(expected).map_err(|_| IndexError::Invalid)?;
    let mut file = tempfile::NamedTempFile::new_in(path).map_err(|_| IndexError::Unavailable)?;
    file.write_all(&bytes).map_err(|_| IndexError::Uncertain)?;
    file
      .as_file()
      .sync_all()
      .map_err(|_| IndexError::Uncertain)?;
    file
      .persist_noclobber(&metadata)
      .map_err(|_| IndexError::Uncertain)?;
    #[cfg(unix)]
    File::open(path)
      .and_then(|directory| directory.sync_all())
      .map_err(|_| IndexError::Uncertain)?;
  }
  // Supplying the vector definition also lets the SDK finish an empty shard
  // directory left by interrupted creation. Existing segments are checked for
  // compatibility by EdgeShard::load; no data is discarded on mismatch.
  let config = EdgeConfigBuilder::new()
    .max_search_threads(4)
    .vector(
      VECTOR,
      EdgeVectorParamsBuilder::new(expected.dimensions as usize, Distance::Cosine).build(),
    )
    .build();
  let shard = if shard_path
    .try_exists()
    .map_err(|_| IndexError::Unavailable)?
  {
    EdgeShard::load(&shard_path, Some(config)).map_err(|_| IndexError::Unavailable)?
  } else {
    let shard = EdgeShard::new(&shard_path, config).map_err(|_| IndexError::Unavailable)?;
    shard.flush().map_err(|_| IndexError::Uncertain)?;
    shard
  };
  {
    let config = shard.config();
    let vector = config.vectors.get(VECTOR).ok_or(IndexError::Unsupported)?;
    if vector.size != expected.dimensions as usize
      || vector.distance != Distance::Cosine
      || !config.wal_enabled()
    {
      return Err(IndexError::Unsupported);
    }
  }
  Ok((shard, lock))
}
