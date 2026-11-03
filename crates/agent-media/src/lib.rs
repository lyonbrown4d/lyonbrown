//! Explicit user image attachments; never discovers, captures or uploads images implicitly.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
pub const MAX_IMAGES: usize = 8;
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_RPC_BYTES: usize = 40 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct ImageAttachment {
  pub mime_type: String,
  pub data: Vec<u8>,
  pub sha256: String,
}
impl std::fmt::Debug for ImageAttachment {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("ImageAttachment")
      .field("mime_type", &self.mime_type)
      .field("bytes", &self.data.len())
      .field("sha256", &self.sha256)
      .finish()
  }
}
impl ImageAttachment {
  pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
    ensure!(
      !data.is_empty() && data.len() <= MAX_IMAGE_BYTES,
      "image byte limit exceeded"
    );
    let format = image::guess_format(&data).context("unsupported image encoding")?;
    let image = Self {
      mime_type: format.to_mime_type().into(),
      sha256: hex::encode(Sha256::digest(&data)),
      data,
    };
    validate_images("", std::slice::from_ref(&image))?;
    Ok(image)
  }
}

pub fn validate_images(prompt: &str, images: &[ImageAttachment]) -> Result<()> {
  ensure!(
    prompt.len() <= 1_048_576 && (!prompt.trim().is_empty() || !images.is_empty()),
    "empty or oversized input"
  );
  ensure!(images.len() <= MAX_IMAGES, "image count limit exceeded");
  let mut total = 0usize;
  for image in images {
    ensure!(
      !image.data.is_empty() && image.data.len() <= MAX_IMAGE_BYTES,
      "image byte limit exceeded"
    );
    total = total
      .checked_add(image.data.len())
      .context("image byte limit exceeded")?;
    ensure!(total <= MAX_TOTAL_BYTES, "total image byte limit exceeded");
    ensure!(
      matches!(
        image.mime_type.as_str(),
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
      ),
      "unsupported image type"
    );
    ensure!(
      image.sha256 == hex::encode(Sha256::digest(&image.data)),
      "image digest mismatch"
    );
    let format = image::guess_format(&image.data).context("unsupported image encoding")?;
    ensure!(
      format.to_mime_type() == image.mime_type,
      "image MIME type mismatch"
    );
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&image.data))
      .with_guessed_format()?
      .into_dimensions()
      .context("invalid image dimensions")?;
    ensure!(
      width > 0
        && height > 0
        && width <= 8192
        && height <= 8192
        && u64::from(width) * u64::from(height) <= 16_777_216,
      "image pixel limit exceeded"
    );
  }
  Ok(())
}

/// Read only the path explicitly selected by the user, with a bounded read even
/// if a file grows after its metadata was checked. No directory discovery.
pub fn read_explicit_image(path: &Path) -> Result<ImageAttachment> {
  ensure!(
    std::fs::metadata(path)?.is_file(),
    "selected image must be a regular file"
  );
  let file = std::fs::File::open(path).context("selected image could not be opened")?;
  ensure!(
    file.metadata()?.is_file(),
    "selected image must be a regular file"
  );
  let mut data = Vec::new();
  file
    .take((MAX_IMAGE_BYTES + 1) as u64)
    .read_to_end(&mut data)?;
  ImageAttachment::from_bytes(data)
}
