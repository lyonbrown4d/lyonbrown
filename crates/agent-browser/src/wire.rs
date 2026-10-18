//! Bounded private pipe framing. Never expose this transport to web content.
use crate::{BrowserError, BrowserResult, Command, Reply};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

pub const MAX_PACKET_BYTES: usize = 18 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
  pub id: u64,
  pub command: Command,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
  pub id: u64,
  pub result: BrowserResult<Reply>,
}

pub fn encode<T: Serialize>(value: &T) -> BrowserResult<Vec<u8>> {
  let bytes =
    postcard::to_stdvec(value).map_err(|error| BrowserError::Protocol(error.to_string()))?;
  if bytes.len() > MAX_PACKET_BYTES {
    return Err(BrowserError::InvalidInput(
      "host packet exceeds 18 MiB".into(),
    ));
  }
  Ok(bytes)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> BrowserResult<T> {
  if bytes.len() > MAX_PACKET_BYTES {
    return Err(BrowserError::InvalidInput(
      "host packet exceeds 18 MiB".into(),
    ));
  }
  postcard::from_bytes(bytes).map_err(|error| BrowserError::Protocol(error.to_string()))
}

pub fn read_packet<T: DeserializeOwned>(reader: &mut impl Read) -> BrowserResult<T> {
  let mut length = [0; 4];
  reader
    .read_exact(&mut length)
    .map_err(|error| BrowserError::Protocol(error.to_string()))?;
  let length = u32::from_le_bytes(length) as usize;
  if length > MAX_PACKET_BYTES {
    return Err(BrowserError::InvalidInput(
      "host packet exceeds 18 MiB".into(),
    ));
  }
  let mut bytes = vec![0; length];
  reader
    .read_exact(&mut bytes)
    .map_err(|error| BrowserError::Protocol(error.to_string()))?;
  decode(&bytes)
}

pub fn write_packet<T: Serialize>(writer: &mut impl Write, value: &T) -> BrowserResult<()> {
  let bytes = encode(value)?;
  writer
    .write_all(&(bytes.len() as u32).to_le_bytes())
    .and_then(|()| writer.write_all(&bytes))
    .and_then(|()| writer.flush())
    .map_err(|error| BrowserError::Protocol(error.to_string()))
}
