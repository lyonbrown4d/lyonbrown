use crate::FileEntry;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct DiscoveryReport {
  pub truncated: bool,
  pub visited_entries: usize,
  pub searched_files: usize,
  pub read_bytes: usize,
  pub skipped_symlinks: usize,
  pub skipped_directories: usize,
  pub skipped_binary: usize,
  pub skipped_large: usize,
  pub skipped_unreadable: usize,
  pub skipped_non_utf8: usize,
  pub depth_limited: usize,
  pub entry_limit_reached: bool,
  pub result_limit_reached: bool,
  pub content_limit_reached: bool,
  pub output_limit_reached: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct FileListing {
  pub entries: Vec<FileEntry>,
  pub report: DiscoveryReport,
}
#[derive(Debug, Clone, Serialize)]
pub struct ContentMatch {
  pub path: String,
  pub line: usize,
  pub text: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct FileSearch {
  pub matches: Vec<ContentMatch>,
  pub report: DiscoveryReport,
}
