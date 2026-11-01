//! Literal-loopback Hugging Face metadata and synthetic artifact fixture.
use super::Harness;
use gpui_kit::{
  AppContext as _, ElementId, ScrollDelta, TestAppContext, point, px, test::TestWindowExt as _,
};
use serde_json::json;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const REVISION: &str = "0123456789012345678901234567890123456789";
pub const ARTIFACT: &[u8] = b"GGUF-synthetic-model-bytes";
const DIGEST: &str = "9ff804ed3e4645794c1b78ac5712e8cf670e0b027633c2fbd66ac52a26f4dde2";

pub struct Hub {
  pub endpoint: String,
  pub downloads: Arc<AtomicUsize>,
  _runtime: Arc<tokio::runtime::Runtime>,
  serving: tokio::task::JoinHandle<()>,
}
impl Hub {
  pub fn start() -> Self {
    let runtime = Arc::new(
      tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap(),
    );
    let listener = runtime
      .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
      .unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let downloads = Arc::new(AtomicUsize::new(0));
    let counts = downloads.clone();
    let serving = runtime.spawn(async move {
      loop {
        let Ok((mut socket, _)) = listener.accept().await else { break; };
        let counts = counts.clone();
        tokio::spawn(async move {
          let mut request = Vec::new();
          loop {
            let mut chunk = [0; 2048];
            let Ok(count) = socket.read(&mut chunk).await else { return; };
            if count == 0 { return; }
            request.extend_from_slice(&chunk[..count]);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") { break; }
            assert!(request.len() <= 16384, "bounded synthetic Hub request headers");
          }
          let request = String::from_utf8_lossy(&request);
          let mut line = request.lines().next().unwrap_or_default().split_whitespace();
          let method = line.next().unwrap_or_default();
          let path = line.next().unwrap_or_default();
          let artifact = path.contains("/resolve/");
          let info = json!({"id":"test/model","sha":REVISION,"gated":false,"config":{"model_type":"llama"},"cardData":{"license":"mit"}});
          let body = if artifact {
            if method == "GET" { counts.fetch_add(1, Ordering::SeqCst); }
            ARTIFACT.to_vec()
          } else if path.split('?').next() == Some("/api/models") {
            serde_json::to_vec(&json!([info])).unwrap()
          } else if path.contains("/tree/") {
            serde_json::to_vec(&json!([{"type":"file","oid":"fixture","path":"model.gguf","size":ARTIFACT.len(),"lfs":{"sha256":DIGEST,"size":ARTIFACT.len(),"pointer_size":100}}])).unwrap()
          } else {
            serde_json::to_vec(&info).unwrap()
          };
          let headers = format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nETag: \"{DIGEST}\"\r\nX-Repo-Commit: {REVISION}\r\nConnection: close\r\n\r\n", if artifact { "application/octet-stream" } else { "application/json" }, body.len());
          socket.write_all(headers.as_bytes()).await.unwrap();
          if method != "HEAD" { socket.write_all(&body).await.unwrap(); }
        });
      }
    });
    Self {
      endpoint,
      downloads,
      _runtime: runtime,
      serving,
    }
  }
}
impl Drop for Hub {
  fn drop(&mut self) {
    self.serving.abort();
  }
}

pub fn click(harness: &Harness, id: impl Into<ElementId>, cx: &mut TestAppContext) {
  let id = id.into();
  reveal(harness, id.clone(), cx);
  harness.click(id, cx);
}
pub fn input(harness: &Harness, id: impl Into<ElementId>, value: &str, cx: &mut TestAppContext) {
  let id = id.into();
  reveal(harness, id.clone(), cx);
  cx.update_window(harness.window.into(), |_, window, cx| {
    window.click(id, cx);
    window.press(&crate::support::shortcut("a"), cx);
    window.input(value, cx);
  })
  .unwrap();
}
fn reveal(harness: &Harness, id: ElementId, cx: &mut TestAppContext) {
  cx.update_window(harness.window.into(), |_, window, cx| {
    for _ in 0..8 {
      window.render_frame(cx);
      let target = window.find(id.clone());
      let viewport = window.find("resource-content").bounds();
      let bounds = target.bounds();
      if target.visible() && bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom()
      {
        return;
      }
      let delta = (viewport.top() + viewport.bottom() - bounds.top() - bounds.bottom()) / 2.;
      window.scroll(
        "resource-content",
        ScrollDelta::Pixels(point(px(0.), delta)),
        cx,
      );
    }
    assert!(
      window.find(id).visible(),
      "Hub control must be reachable through resource scroll"
    );
  })
  .unwrap();
}
