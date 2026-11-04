//! Standard user image parts are opaque to text masking, never to validation.
use crate::GatewayError;
use agent_media::{ImageAttachment, MAX_IMAGE_BYTES, MAX_IMAGES, MAX_TOTAL_BYTES};
use base64::Engine;
use serde_json::{Value, json};

pub(crate) struct InlineImages(Vec<(usize, usize, Value)>);
impl InlineImages {
  /// Runs inside the content policy's bounded blocking worker. Only exact user
  /// image parts are removed; an object in tool arguments cannot claim this path.
  pub fn take(body: &mut Value) -> Result<Self, GatewayError> {
    let mut images = Vec::new();
    let mut bytes = 0usize;
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
      for (message_index, message) in messages.iter_mut().enumerate() {
        let user = message["role"] == "user";
        let Some(parts) = message.get_mut("content").and_then(Value::as_array_mut) else {
          continue;
        };
        for (part_index, part) in parts.iter_mut().enumerate() {
          if part["type"] != "image_url" {
            continue;
          }
          if !user || images.len() >= MAX_IMAGES {
            return Err(invalid());
          }
          bytes = bytes
            .checked_add(validate_part(part)?.data.len())
            .ok_or_else(invalid)?;
          if bytes > MAX_TOTAL_BYTES {
            return Err(invalid());
          }
          let original = std::mem::replace(part, json!({"type":"text", "text":"[inline image]"}));
          images.push((message_index, part_index, original));
        }
      }
    }
    Ok(Self(images))
  }
  /// The text policy may change text values only. A structural mismatch refuses
  /// the request rather than restoring image data into a different destination.
  pub fn restore(self, body: &mut Value) -> Result<(), GatewayError> {
    for (message, part, image) in self.0 {
      let target = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .and_then(|messages| messages.get_mut(message))
        .and_then(|message| message.get_mut("content"))
        .and_then(Value::as_array_mut)
        .and_then(|parts| parts.get_mut(part))
        .ok_or_else(invalid)?;
      if target["type"] != "text" {
        return Err(invalid());
      }
      *target = image;
    }
    Ok(())
  }
}
fn only(value: &Value, fields: &[&str]) -> bool {
  value
    .as_object()
    .is_some_and(|object| object.keys().all(|key| fields.contains(&key.as_str())))
}
pub(crate) fn validate_part(part: &Value) -> Result<ImageAttachment, GatewayError> {
  if !only(part, &["type", "image_url"]) || part["type"] != "image_url" {
    return Err(invalid());
  }
  let image = &part["image_url"];
  if !only(image, &["url", "detail"])
    || image
      .get("detail")
      .is_some_and(|detail| !matches!(detail.as_str(), Some("auto" | "low" | "high")))
  {
    return Err(invalid());
  }
  let url = image["url"].as_str().ok_or_else(invalid)?;
  let (mime, data) = url
    .strip_prefix("data:")
    .and_then(|url| url.split_once(";base64,"))
    .ok_or_else(invalid)?;
  if !matches!(
    mime,
    "image/png" | "image/jpeg" | "image/webp" | "image/gif"
  ) || data.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4
  {
    return Err(invalid());
  }
  let bytes = base64::engine::general_purpose::STANDARD
    .decode(data)
    .map_err(|_| invalid())?;
  let attachment = ImageAttachment::from_bytes(bytes).map_err(|_| invalid())?;
  if attachment.mime_type != mime {
    return Err(invalid());
  }
  Ok(attachment)
}
fn invalid() -> GatewayError {
  GatewayError::invalid(
    "Image content requires bounded, correctly encoded inline PNG, JPEG, WebP or GIF data",
  )
}
