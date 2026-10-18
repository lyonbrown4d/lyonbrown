use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::Duration,
};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  sync::Notify,
  task::{JoinHandle, JoinSet},
};

pub struct PoisonPage {
  pub url: String,
  effects: Arc<AtomicUsize>,
  changed: Arc<Notify>,
  server: JoinHandle<()>,
}

impl PoisonPage {
  pub async fn start(label: &'static str) -> Self {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let effects = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(Notify::new());
    let recorded = effects.clone();
    let notify = changed.clone();
    let html = Arc::new(format!(
      r#"<!doctype html><meta charset="utf-8"><title>Synthetic human input poison</title><button style="position:fixed;left:20px;top:20px;width:160px;height:50px" onclick="fetch('/effect',{{method:'POST',keepalive:true}});this.setAttribute('aria-label','x'.repeat({}))">{label}</button>"#,
      agent_browser::MAX_AX_BYTES + 1024
    ));
    let server = tokio::spawn(async move {
      let mut clients = JoinSet::new();
      while let Ok((mut socket, _)) = listener.accept().await {
        while clients.try_join_next().is_some() {}
        let recorded = recorded.clone();
        let notify = notify.clone();
        let html = html.clone();
        clients.spawn(async move {
          let request = tokio::time::timeout(Duration::from_secs(2), async {
            let mut bytes = Vec::new();
            let mut chunk = [0; 1024];
            while bytes.len() < 8192 {
              let count = socket.read(&mut chunk).await.ok()?;
              if count == 0 { return None; }
              bytes.extend_from_slice(&chunk[..count]);
              if bytes.windows(4).any(|part| part == b"\r\n\r\n") { return Some(bytes); }
            }
            None
          }).await;
          let Ok(Some(bytes)) = request else { return };
          let request = String::from_utf8_lossy(&bytes);
          let first = request.lines().next().unwrap_or("");
          let body = if first.starts_with("POST /effect ") {
            recorded.fetch_add(1, Ordering::SeqCst);
            notify.notify_waiters();
            "ok"
          } else if first.starts_with("GET / ") {
            html.as_str()
          } else { "" };
          let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html;charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
          );
          let _ = tokio::time::timeout(Duration::from_secs(2), socket.write_all(response.as_bytes())).await;
        });
      }
    });
    Self {
      url,
      effects,
      changed,
      server,
    }
  }

  pub fn count(&self) -> usize {
    self.effects.load(Ordering::SeqCst)
  }

  pub async fn wait_effect(&self) -> bool {
    tokio::time::timeout(Duration::from_secs(20), async {
      loop {
        let changed = self.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if self.count() > 0 {
          return;
        }
        changed.await;
      }
    })
    .await
    .is_ok()
  }
}

impl Drop for PoisonPage {
  fn drop(&mut self) {
    self.server.abort();
  }
}
