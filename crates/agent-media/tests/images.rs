use agent_media::{ImageAttachment, MAX_IMAGES, read_explicit_image, validate_images};
const PNG: &[u8] = include_bytes!("fixtures/one.png");
#[test]
fn explicit_images_validate_bytes_mime_digest_and_boundaries() -> anyhow::Result<()> {
  let image = ImageAttachment::from_bytes(PNG.to_vec())?;
  assert_eq!(image.mime_type, "image/png");
  validate_images("", std::slice::from_ref(&image))?;
  let mut wrong = image.clone();
  wrong.mime_type = "image/jpeg".into();
  assert!(validate_images("", &[wrong]).is_err());
  let mut wrong = image.clone();
  wrong.sha256 = "0".repeat(64);
  assert!(validate_images("", &[wrong]).is_err());
  assert!(validate_images("", &vec![image.clone(); MAX_IMAGES + 1]).is_err());
  assert!(ImageAttachment::from_bytes(b"not an image".to_vec()).is_err());
  assert!(!format!("{image:?}").contains(&format!("{PNG:?}")));
  Ok(())
}
#[test]
fn only_the_explicit_regular_file_is_read() -> anyhow::Result<()> {
  let root = tempfile::tempdir()?;
  std::fs::write(root.path().join("chosen.png"), PNG)?;
  std::fs::write(root.path().join("unselected"), b"not an image")?;
  assert_eq!(
    read_explicit_image(&root.path().join("chosen.png"))?.data,
    PNG
  );
  assert!(read_explicit_image(root.path()).is_err());
  Ok(())
}
