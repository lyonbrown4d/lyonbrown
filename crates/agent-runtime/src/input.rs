use crate::{Result, RuntimeError, RuntimeImage};
use base64::Engine;

pub(crate) fn validate(prompt: &str, images: &[RuntimeImage]) -> Result<()> {
  if prompt.len() > 1_048_576 || (prompt.trim().is_empty() && images.is_empty()) || images.len() > 8
  {
    return Err(RuntimeError::Configuration(
      "invalid prompt or image count".into(),
    ));
  }
  let mut total = 0_usize;
  for image in images {
    if !matches!(
      image.mime_type.as_str(),
      "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    ) || image.data_base64.len() > 24 * 1024 * 1024
    {
      return Err(RuntimeError::Configuration(
        "unsupported image type or image exceeds 16MiB".into(),
      ));
    }
    let decoded = base64::engine::general_purpose::STANDARD
      .decode(&image.data_base64)
      .map_err(|_| RuntimeError::Configuration("image data is not valid base64".into()))?;
    total = total.saturating_add(decoded.len());
    if decoded.is_empty() || decoded.len() > 16 * 1024 * 1024 || total > 32 * 1024 * 1024 {
      return Err(RuntimeError::Configuration(
        "image byte limit exceeded".into(),
      ));
    }
    let format = image::guess_format(&decoded)
      .map_err(|_| RuntimeError::Configuration("unsupported image encoding".into()))?;
    if format.to_mime_type() != image.mime_type {
      return Err(RuntimeError::Configuration(
        "image MIME type does not match its encoding".into(),
      ));
    }
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(decoded))
      .with_guessed_format()
      .map_err(|_| RuntimeError::Configuration("invalid image header".into()))?
      .into_dimensions()
      .map_err(|_| RuntimeError::Configuration("invalid image dimensions".into()))?;
    if width == 0
      || height == 0
      || width > 8192
      || height > 8192
      || u64::from(width) * u64::from(height) > 16_777_216
    {
      return Err(RuntimeError::Configuration(
        "image pixel limit exceeded".into(),
      ));
    }
  }
  Ok(())
}
