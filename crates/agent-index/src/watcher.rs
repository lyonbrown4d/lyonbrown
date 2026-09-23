use std::{
  path::PathBuf,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
  },
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::{IndexError, WorkspaceIndex};

pub struct WorkspaceWatcher {
  stop: Arc<AtomicBool>,
  worker: Option<JoinHandle<()>>,
  watcher: Option<RecommendedWatcher>,
}

impl WorkspaceWatcher {
  pub(crate) fn start(index: WorkspaceIndex) -> Result<Self, IndexError> {
    let (sender, receiver) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |event| {
      let _ = sender.send(event);
    })
    .map_err(|error| IndexError::Watcher(error.to_string()))?;
    watcher
      .watch(index.root(), RecursiveMode::Recursive)
      .map_err(|error| IndexError::Watcher(error.to_string()))?;
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = thread::Builder::new()
      .name("workspace-index-watcher".into())
      .spawn(move || watch_loop(index, receiver, worker_stop))?;
    Ok(Self {
      stop,
      worker: Some(worker),
      watcher: Some(watcher),
    })
  }
}

impl Drop for WorkspaceWatcher {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Relaxed);
    self.watcher.take();
    if let Some(worker) = self.worker.take() {
      let _ = worker.join();
    }
  }
}

fn watch_loop(
  index: WorkspaceIndex,
  receiver: mpsc::Receiver<notify::Result<Event>>,
  stop: Arc<AtomicBool>,
) {
  let mut last_reconcile = Instant::now();
  while !stop.load(Ordering::Relaxed) {
    let event = match receiver.recv_timeout(Duration::from_secs(1)) {
      Ok(event) => event,
      Err(mpsc::RecvTimeoutError::Timeout) => {
        if last_reconcile.elapsed() >= Duration::from_secs(300) {
          reconcile(&index);
          last_reconcile = Instant::now();
        }
        continue;
      }
      Err(mpsc::RecvTimeoutError::Disconnected) => break,
    };
    let (mut paths, mut full_rescan) = match event {
      Ok(event) => {
        let rescan = event.need_rescan();
        (event.paths, rescan)
      }
      Err(_) => {
        reconcile(&index);
        continue;
      }
    };
    thread::sleep(Duration::from_millis(100));
    while let Ok(next) = receiver.try_recv() {
      match next {
        Ok(event) => {
          full_rescan |= event.need_rescan();
          paths.extend(event.paths);
        }
        Err(_) => {
          reconcile(&index);
          paths.clear();
          break;
        }
      }
    }
    if full_rescan {
      reconcile(&index);
      last_reconcile = Instant::now();
      continue;
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
      reconcile(&index);
      last_reconcile = Instant::now();
      continue;
    }
    for path in paths {
      let normalized = normalize_event_path(index.root(), path);
      if let Err(error) = index.update_path(&normalized) {
        tracing::warn!(path = %normalized.display(), %error, "workspace index update failed; rescanning");
        reconcile(&index);
        last_reconcile = Instant::now();
        break;
      }
    }
  }
}

fn reconcile(index: &WorkspaceIndex) {
  if let Err(error) = index.reconcile() {
    tracing::warn!(%error, "workspace index reconciliation failed");
  }
}

fn normalize_event_path(root: &std::path::Path, path: PathBuf) -> PathBuf {
  if path.is_absolute() {
    path
  } else {
    root.join(path)
  }
}
