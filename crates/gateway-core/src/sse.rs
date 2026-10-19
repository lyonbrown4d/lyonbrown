use bytes::Bytes;
use rig_core::http_client::framing::SseFramer;
use std::io;

/// Sidecar validates completion while the transport forwards original bytes.
pub(crate) struct SseValidator {
  frames: SseFramer,
  utf8_pending: Vec<u8>,
  pub done: bool,
  saw_choice: bool,
}
impl SseValidator {
  pub fn new() -> Self {
    Self {
      frames: SseFramer::new(),
      utf8_pending: Vec::new(),
      done: false,
      saw_choice: false,
    }
  }
  pub fn check(&mut self, bytes: &Bytes) -> Result<(), io::Error> {
    // Rig's framer intentionally uses lossy UTF-8. Validate first so malformed
    // bytes remain failures, while a codepoint split between chunks stays valid.
    let joined;
    let bytes = if self.utf8_pending.is_empty() {
      bytes.as_ref()
    } else {
      let mut pending = std::mem::take(&mut self.utf8_pending);
      pending.extend_from_slice(bytes);
      joined = pending;
      joined.as_slice()
    };
    let complete = match std::str::from_utf8(bytes) {
      Ok(_) => bytes,
      Err(error) if error.error_len().is_none() => {
        let (complete, pending) = bytes.split_at(error.valid_up_to());
        self.utf8_pending.extend_from_slice(pending);
        complete
      }
      Err(_) => return Err(io::Error::other("Invalid upstream SSE encoding")),
    };
    for event in self.frames.push(complete) {
      if self.done {
        return Err(io::Error::other("Unexpected event after terminal marker"));
      }
      if event.data == "[DONE]" {
        if !self.saw_choice {
          return Err(io::Error::other(
            "Terminal marker without completion choices",
          ));
        }
        self.done = true;
      } else {
        let value: serde_json::Value = serde_json::from_str(&event.data)
          .map_err(|_| io::Error::other("Invalid upstream completion event"))?;
        self.saw_choice |= value
          .get("choices")
          .and_then(serde_json::Value::as_array)
          .is_some_and(|choices| !choices.is_empty());
        if !value.is_object()
          || value.get("error").is_some()
          || !value
            .get("choices")
            .is_some_and(serde_json::Value::is_array)
        {
          return Err(io::Error::other("Invalid upstream completion event"));
        }
      }
    }
    if self.done && (!self.utf8_pending.is_empty() || self.frames.pending() != 0) {
      return Err(io::Error::other(
        "Unexpected data after upstream terminal marker",
      ));
    }
    Ok(())
  }
}
