use anyhow::Result;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, event::ModifyKind};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer_opt};
use std::{
  path::{Path, PathBuf},
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};
use tokio::sync::mpsc;
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceChange {
  pub kind: String,
  pub paths: Vec<String>,
}
pub struct WorkspaceWatch {
  _watcher: Debouncer<RecommendedWatcher, RecommendedCache>,
  receiver: mpsc::Receiver<WorkspaceChange>,
  overflow: Arc<AtomicBool>,
}
impl WorkspaceWatch {
  pub fn open(workspace: &Path) -> Result<Self> {
    let root = std::fs::canonicalize(workspace)?;
    anyhow::ensure!(root.is_dir(), "watch workspace must be a directory");
    let (sender, receiver) = mpsc::channel(256);
    let overflow = Arc::new(AtomicBool::new(false));
    let dropped = overflow.clone();
    let mut watcher = new_debouncer_opt::<_, RecommendedWatcher, RecommendedCache>(
      Duration::from_millis(150),
      None,
      move |result: DebounceEventResult| match result {
        Ok(events) => {
          for event in events {
            let kind = if event.need_rescan() {
              "rescan"
            } else {
              match event.kind {
                EventKind::Create(_) => "created",
                EventKind::Remove(_) => "removed",
                EventKind::Modify(ModifyKind::Name(_)) => "renamed",
                EventKind::Modify(_) => "modified",
                EventKind::Access(_) => continue,
                _ => "rescan",
              }
            };
            let paths = relative_paths(&root, &event.paths);
            if paths.is_empty() && kind != "rescan" {
              continue;
            }
            if sender
              .try_send(WorkspaceChange {
                kind: kind.into(),
                paths,
              })
              .is_err()
            {
              dropped.store(true, Ordering::Release);
            }
          }
        }
        Err(_) => {
          if sender
            .try_send(WorkspaceChange {
              kind: "watch_error".into(),
              paths: Vec::new(),
            })
            .is_err()
          {
            dropped.store(true, Ordering::Release);
          }
        }
      },
      RecommendedCache::new(),
      notify::Config::default().with_follow_symlinks(false),
    )?;
    watcher.watch(workspace, RecursiveMode::Recursive)?;
    Ok(Self {
      _watcher: watcher,
      receiver,
      overflow,
    })
  }
  pub async fn next(&mut self) -> Option<WorkspaceChange> {
    if self.overflow.swap(false, Ordering::AcqRel) {
      return Some(WorkspaceChange {
        kind: "rescan".into(),
        paths: Vec::new(),
      });
    }
    self.receiver.recv().await
  }
}
fn relative_paths(root: &Path, paths: &[PathBuf]) -> Vec<String> {
  paths
    .iter()
    .filter_map(|path| path.strip_prefix(root).ok())
    .filter(|path| {
      !path.components().any(|part| {
        matches!(
          part.as_os_str().to_str(),
          Some(".git" | "target" | "node_modules")
        )
      })
    })
    .map(|path| path.to_string_lossy().replace('\\', "/"))
    .collect()
}
