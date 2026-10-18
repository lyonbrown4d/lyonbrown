//! Bounded private pipe framing. Authentication is supplied by the embedding host.
//! Serialized commands/replies contain private screen and input data; never log.
use std::io::{Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{Command, Reply, RuntimeError, RuntimeResult};

pub const MAX_PACKET_BYTES: usize = 68 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Request {
  pub id: u64,
  pub command: Command,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Response {
  pub id: u64,
  pub result: RuntimeResult<Reply>,
}

pub fn encode<T: Serialize>(value: &T) -> RuntimeResult<Vec<u8>> {
  // Count first so oversized values do not allocate an oversized packet.
  let size = postcard::experimental::serialized_size(value).map_err(|_| RuntimeError::Protocol)?;
  check_size(size)?;
  let bytes = postcard::to_stdvec(value).map_err(|_| RuntimeError::Protocol)?;
  check_size(bytes.len())?;
  Ok(bytes)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> RuntimeResult<T> {
  check_size(bytes.len())?;
  let (value, remaining) = postcard::take_from_bytes(bytes).map_err(|_| RuntimeError::Protocol)?;
  if !remaining.is_empty() {
    return Err(RuntimeError::Protocol);
  }
  Ok(value)
}

pub fn read_packet<T: DeserializeOwned>(reader: &mut impl Read) -> RuntimeResult<T> {
  let mut prefix = [0; 4];
  reader
    .read_exact(&mut prefix)
    .map_err(|_| RuntimeError::Protocol)?;
  let length = u32::from_le_bytes(prefix) as usize;
  check_size(length)?;
  let mut bytes = vec![0; length];
  reader
    .read_exact(&mut bytes)
    .map_err(|_| RuntimeError::Protocol)?;
  decode(&bytes)
}

pub fn write_packet<T: Serialize>(writer: &mut impl Write, value: &T) -> RuntimeResult<()> {
  let bytes = encode(value)?;
  let length = u32::try_from(bytes.len()).map_err(|_| RuntimeError::PacketTooLarge)?;
  writer
    .write_all(&length.to_le_bytes())
    .and_then(|()| writer.write_all(&bytes))
    .and_then(|()| writer.flush())
    .map_err(|_| RuntimeError::Protocol)
}

fn check_size(length: usize) -> RuntimeResult<()> {
  if length > MAX_PACKET_BYTES {
    return Err(RuntimeError::PacketTooLarge);
  }
  Ok(())
}
