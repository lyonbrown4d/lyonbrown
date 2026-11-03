//! Explicit native file selection; attachments remain scoped to the requesting tab.
use crate::{Desktop, connection::Update};
use gpui_kit::*;
use std::path::PathBuf;

impl Desktop {
  pub(crate) fn add_selected_images(
    &mut self,
    scope: String,
    images: Vec<agent_client::media::NamedImage>,
  ) {
    if self.persistence_scope.as_deref() != Some(scope.as_str()) {
      return;
    }
    let mut proposed = self.vm.images.clone();
    for image in images {
      if let Err(error) = proposed.add(image.name, image.image) {
        self.vm.error = Some(error.to_string());
        return;
      }
    }
    self.vm.images = proposed;
  }

  pub(crate) fn choose_images(&mut self, cx: &mut Context<Self>) {
    if self.client.is_none()
      || self.vm.busy
      || self.vm.active_run.is_some()
      || self.vm.images.pending.is_some()
      || self.navigation.connecting
      || self.persistence_loading
    {
      return;
    }
    let Some(scope) = self.persistence_scope.clone() else {
      return;
    };
    let selection = cx.prompt_for_paths(PathPromptOptions {
      files: true,
      directories: false,
      multiple: true,
      prompt: Some("选择要发送给模型的图片".into()),
    });
    let sender = self.scoped_sender();
    let runtime = self.runtime.clone();
    cx.spawn(async move |_, _| match selection.await {
      Ok(Ok(Some(paths))) => {
        let request = runtime.spawn_blocking(move || read_selected(paths));
        let reply = match request.await {
          Ok(Ok(images)) => Update::ImagesSelected { scope, images },
          _ => Update::Error(
            "图片读取失败；支持 PNG、JPEG、WebP、GIF，最多 8 张、单张 16 MiB、合计 32 MiB。".into(),
          ),
        };
        sender.send(reply).await;
      }
      Ok(Ok(None)) => {}
      _ => {
        sender
          .send(Update::Error(
            "系统文件选择器不可用；没有读取或发送图片。".into(),
          ))
          .await;
      }
    })
    .detach();
  }
  pub(crate) fn remove_image(&mut self, index: usize, cx: &mut Context<Self>) {
    if self.vm.busy {
      return;
    }
    if let Err(error) = self.vm.images.remove(index) {
      self.vm.error = Some(error.to_string());
    }
    cx.notify();
  }
}
fn read_selected(paths: Vec<PathBuf>) -> anyhow::Result<Vec<agent_client::media::NamedImage>> {
  anyhow::ensure!(
    !paths.is_empty() && paths.len() <= agent_media::MAX_IMAGES,
    "image count limit exceeded"
  );
  let mut images = Vec::with_capacity(paths.len());
  for path in paths {
    let name = path
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .unwrap_or_else(|| "image".into());
    images.push(agent_client::media::NamedImage {
      name,
      image: agent_media::read_explicit_image(&path)?,
    });
    let bytes = images
      .iter()
      .map(|entry| entry.image.data.len())
      .sum::<usize>();
    anyhow::ensure!(
      bytes <= agent_media::MAX_TOTAL_BYTES,
      "image byte limit exceeded"
    );
  }
  Ok(images)
}
