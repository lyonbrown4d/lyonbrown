use std::{
  collections::HashSet,
  path::{Component, Path, PathBuf},
  sync::Arc,
  time::Duration,
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{scan, watcher::WorkspaceWatcher};

const MAX_SEARCH_LIMIT: usize = 50;
const MAX_SNIPPET_CHARS: usize = 1_200;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
  #[error("workspace index I/O failed: {0}")]
  Io(#[from] std::io::Error),
  #[error("workspace index database failed: {0}")]
  Database(#[from] rusqlite::Error),
  #[error("workspace watcher failed: {0}")]
  Watcher(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedDocument {
  pub path: PathBuf,
  pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceIndex {
  root: Arc<PathBuf>,
  database: Arc<PathBuf>,
}

impl WorkspaceIndex {
  pub fn open(root: impl AsRef<Path>, database: impl AsRef<Path>) -> Result<Self, IndexError> {
    let index = Self::initialize_for(root.as_ref(), database.as_ref())?;
    index.reconcile()?;
    Ok(index)
  }

  pub fn open_and_watch(
    root: impl AsRef<Path>,
    database: impl AsRef<Path>,
  ) -> Result<(Self, WorkspaceWatcher), IndexError> {
    let index = Self::initialize_for(root.as_ref(), database.as_ref())?;
    let watcher = WorkspaceWatcher::start(index.clone())?;
    index.reconcile()?;
    Ok((index, watcher))
  }

  fn initialize_for(root: &Path, database: &Path) -> Result<Self, IndexError> {
    let root = root.canonicalize()?;
    let database = database.to_owned();
    if let Some(parent) = database.parent() {
      std::fs::create_dir_all(parent)?;
    }
    let index = Self {
      root: Arc::new(root),
      database: Arc::new(database),
    };
    index.initialize()?;
    Ok(index)
  }

  pub fn search(&self, query: &str, limit: usize) -> Result<Vec<IndexedDocument>, IndexError> {
    let query = fts_query(query);
    if query.is_empty() || limit == 0 {
      return Ok(Vec::new());
    }
    let connection = self.connect()?;
    let mut statement = connection.prepare(
      "SELECT path, snippet(workspace_fts, 1, '[', ']', '…', 12)
       FROM workspace_fts WHERE workspace_fts MATCH ?1 LIMIT ?2",
    )?;
    let rows = statement.query_map(params![query, limit.min(MAX_SEARCH_LIMIT) as i64], |row| {
      Ok(IndexedDocument {
        path: PathBuf::from(row.get::<_, String>(0)?),
        snippet: truncate_chars(row.get(1)?, MAX_SNIPPET_CHARS),
      })
    })?;
    rows
      .collect::<Result<Vec<_>, _>>()
      .map_err(IndexError::from)
  }

  pub fn reconcile(&self) -> Result<(), IndexError> {
    let mut seen = HashSet::new();
    let files = scan::workspace_files(&self.root)?;
    let mut connection = self.connect()?;
    let transaction = connection.transaction()?;
    for (relative, content) in files {
      let path = relative.to_string_lossy().replace('\\', "/");
      seen.insert(path.clone());
      upsert_in(&transaction, &path, &content)?;
    }
    let stale = {
      let mut statement = transaction.prepare("SELECT path FROM indexed_files")?;
      statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| !seen.contains(path))
        .collect::<Vec<_>>()
    };
    for path in stale {
      transaction.execute("DELETE FROM workspace_fts WHERE path = ?1", [&path])?;
      transaction.execute("DELETE FROM indexed_files WHERE path = ?1", [&path])?;
    }
    transaction.commit()?;
    Ok(())
  }

  pub fn watch(&self) -> Result<WorkspaceWatcher, IndexError> {
    let watcher = WorkspaceWatcher::start(self.clone())?;
    self.reconcile()?;
    Ok(watcher)
  }

  pub(crate) fn root(&self) -> &Path {
    &self.root
  }

  pub(crate) fn database(&self) -> &Path {
    &self.database
  }

  pub(crate) fn update_path(&self, absolute: &Path) -> Result<(), IndexError> {
    if !absolute.starts_with(self.root()) || absolute == self.root() {
      return Ok(());
    }
    if absolute.is_dir() || !absolute.exists() {
      return self.reconcile();
    }
    let relative = absolute.strip_prefix(self.root()).unwrap_or(absolute);
    if relative
      .components()
      .any(|component| !matches!(component, Component::Normal(_)))
    {
      return Ok(());
    }
    if matches!(
      relative.file_name().and_then(|name| name.to_str()),
      Some(".gitignore" | ".ignore")
    ) {
      return self.reconcile();
    }
    if let Some(content) = scan::read_indexable(self.root(), relative)? {
      let Some(path) = relative.to_str() else {
        return Ok(());
      };
      let path = path.replace('\\', "/");
      self.upsert(&path, &content)?;
    } else {
      let Some(path) = relative.to_str() else {
        return Ok(());
      };
      let path = path.replace('\\', "/");
      let connection = self.connect()?;
      connection.execute("DELETE FROM workspace_fts WHERE path = ?1", [&path])?;
      connection.execute("DELETE FROM indexed_files WHERE path = ?1", [&path])?;
    }
    Ok(())
  }

  fn initialize(&self) -> Result<(), IndexError> {
    let connection = self.connect()?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.execute_batch(
      "CREATE TABLE IF NOT EXISTS indexed_files (
         path TEXT PRIMARY KEY,
         content_hash TEXT NOT NULL
       );
       CREATE VIRTUAL TABLE IF NOT EXISTS workspace_fts USING fts5(path UNINDEXED, content);",
    )?;
    Ok(())
  }

  fn connect(&self) -> Result<Connection, IndexError> {
    let connection = Connection::open(self.database())?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
  }

  fn upsert(&self, path: &str, content: &str) -> Result<(), IndexError> {
    let mut connection = self.connect()?;
    let transaction = connection.transaction()?;
    upsert_in(&transaction, path, content)?;
    transaction.commit()?;
    Ok(())
  }
}

fn upsert_in(transaction: &Transaction<'_>, path: &str, content: &str) -> Result<(), IndexError> {
  let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
  let existing = transaction
    .query_row(
      "SELECT content_hash FROM indexed_files WHERE path = ?1",
      [path],
      |row| row.get::<_, String>(0),
    )
    .optional()?;
  if existing.as_deref() == Some(hash.as_str()) {
    return Ok(());
  }
  transaction.execute("DELETE FROM workspace_fts WHERE path = ?1", [path])?;
  transaction.execute(
    "INSERT INTO indexed_files(path, content_hash) VALUES (?1, ?2)
     ON CONFLICT(path) DO UPDATE SET content_hash = excluded.content_hash",
    params![path, hash],
  )?;
  transaction.execute(
    "INSERT INTO workspace_fts(path, content) VALUES (?1, ?2)",
    params![path, content],
  )?;
  Ok(())
}

fn fts_query(query: &str) -> String {
  query
    .split_whitespace()
    .filter(|term| !term.is_empty())
    .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
    .collect::<Vec<_>>()
    .join(" OR ")
}

fn truncate_chars(mut value: String, limit: usize) -> String {
  if let Some((index, _)) = value.char_indices().nth(limit) {
    value.truncate(index);
    value.push('…');
  }
  value
}
