//! Bounded, capability-relative discovery. Omitted content is reported explicitly.
mod types;
use crate::{FileEntry, Workspace, validate_path};
use anyhow::{Result, ensure};
use std::{collections::VecDeque, io::Read};
pub use types::*;

const ENTRY_LIMIT: usize = 2000;
const FILE_LIMIT: u64 = 1_048_576;
const CONTENT_LIMIT: usize = 8 * 1_048_576;
const OUTPUT_LIMIT: usize = 256 * 1024;

impl Workspace {
  /// Root is depth zero; results at max_depth are included but not descended into.
  pub fn list_files(
    &self,
    path: &str,
    max_depth: usize,
    max_results: usize,
  ) -> Result<FileListing> {
    let mut listing = FileListing {
      entries: Vec::new(),
      report: DiscoveryReport::default(),
    };
    let mut output_bytes = 0;
    self.discover(
      path,
      max_depth,
      max_results,
      &mut listing.report,
      |entry, report| {
        let size = serde_json_size(&entry.path) + 100;
        if output_bytes + size > OUTPUT_LIMIT {
          report.output_limit_reached = true;
          return Ok(false);
        }
        output_bytes += size;
        listing.entries.push(entry);
        if listing.entries.len() >= max_results {
          report.result_limit_reached = true;
          return Ok(false);
        }
        Ok(true)
      },
    )?;
    Ok(listing)
  }

  /// Literal, case-sensitive UTF-8 line search, without following symlinks.
  pub fn search_files(
    &self,
    path: &str,
    query: &str,
    max_depth: usize,
    max_results: usize,
  ) -> Result<FileSearch> {
    ensure!(
      !query.is_empty() && query.len() <= 4096 && !query.contains(['\n', '\r', '\0']),
      "query must be a nonempty single-line literal of at most 4096 bytes"
    );
    let mut search = FileSearch {
      matches: Vec::new(),
      report: DiscoveryReport::default(),
    };
    let mut output_bytes = 0;
    self.discover(
      path,
      max_depth,
      max_results,
      &mut search.report,
      |entry, report| {
        if entry.directory {
          return Ok(true);
        }
        if entry.size > FILE_LIMIT {
          report.skipped_large += 1;
          return Ok(true);
        }
        let remaining = CONTENT_LIMIT.saturating_sub(report.read_bytes);
        if entry.size > remaining as u64 {
          report.content_limit_reached = true;
          return Ok(false);
        }
        let mut bytes = Vec::new();
        // cap-std also confines a target replaced with a symlink during traversal.
        let read = self.dir.open(&entry.path).and_then(|file| {
          file
            .take((FILE_LIMIT + 1).min(remaining as u64 + 1))
            .read_to_end(&mut bytes)
        });
        if read.is_err() {
          report.skipped_unreadable += 1;
          return Ok(true);
        }
        report.read_bytes += bytes.len();
        if report.read_bytes > CONTENT_LIMIT {
          report.content_limit_reached = true;
          return Ok(false);
        }
        if bytes.len() > FILE_LIMIT as usize {
          report.skipped_large += 1;
          return Ok(true);
        }
        if bytes.contains(&0) {
          report.skipped_binary += 1;
          return Ok(true);
        }
        let Ok(text) = String::from_utf8(bytes) else {
          report.skipped_non_utf8 += 1;
          return Ok(true);
        };
        report.searched_files += 1;
        for (index, line) in text.lines().enumerate() {
          if !line.contains(query) {
            continue;
          }
          let size = serde_json_size(&entry.path) + serde_json_size(line) + 100;
          if output_bytes + size > OUTPUT_LIMIT {
            report.output_limit_reached = true;
            return Ok(false);
          }
          output_bytes += size;
          search.matches.push(ContentMatch {
            path: entry.path.clone(),
            line: index + 1,
            text: line.into(),
          });
          if search.matches.len() >= max_results {
            report.result_limit_reached = true;
            return Ok(false);
          }
        }
        Ok(true)
      },
    )?;
    Ok(search)
  }

  fn discover(
    &self,
    path: &str,
    max_depth: usize,
    max_results: usize,
    report: &mut DiscoveryReport,
    mut visit: impl FnMut(FileEntry, &mut DiscoveryReport) -> Result<bool>,
  ) -> Result<()> {
    ensure!(
      (1..=8).contains(&max_depth),
      "max_depth must be between 1 and 8"
    );
    ensure!(
      (1..=200).contains(&max_results),
      "max_results must be between 1 and 200"
    );
    let path = if path.is_empty() { "." } else { path };
    validate_path(path)?;
    // Validate the caller-selected root eagerly; traversal errors are explicit omissions.
    self.dir.open_dir(path)?;
    let mut pending = VecDeque::from([(path.to_owned(), 0)]);
    'walk: while let Some((directory, depth)) = pending.pop_front() {
      let entries = match self.dir.open_dir(&directory).and_then(|dir| dir.entries()) {
        Ok(entries) => entries,
        Err(_) => {
          report.skipped_unreadable += 1;
          continue;
        }
      };
      let mut children = Vec::new();
      for entry in entries {
        if report.visited_entries >= ENTRY_LIMIT {
          report.entry_limit_reached = true;
          break;
        }
        report.visited_entries += 1;
        let entry = match entry {
          Ok(entry) => entry,
          Err(_) => {
            report.skipped_unreadable += 1;
            continue;
          }
        };
        let kind = match entry.file_type() {
          Ok(kind) => kind,
          Err(_) => {
            report.skipped_unreadable += 1;
            continue;
          }
        };
        if kind.is_symlink() {
          report.skipped_symlinks += 1;
          continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
          report.skipped_non_utf8 += 1;
          continue;
        };
        if kind.is_dir() && matches!(name.as_str(), ".git" | "node_modules" | "target") {
          report.skipped_directories += 1;
          continue;
        }
        if !kind.is_dir() && !kind.is_file() {
          report.skipped_binary += 1;
          continue;
        }
        let path = if directory == "." {
          name
        } else {
          format!("{directory}/{name}")
        };
        let size = if kind.is_file() {
          match entry.metadata() {
            Ok(meta) => meta.len(),
            Err(_) => {
              report.skipped_unreadable += 1;
              continue;
            }
          }
        } else {
          0
        };
        children.push(FileEntry {
          path,
          directory: kind.is_dir(),
          size,
        });
      }
      children.sort_by(|a, b| a.path.cmp(&b.path));
      for entry in children {
        if entry.directory {
          if depth + 1 < max_depth {
            pending.push_back((entry.path.clone(), depth + 1));
          } else {
            report.depth_limited += 1;
          }
        }
        if !visit(entry, report)? {
          break 'walk;
        }
      }
      if report.entry_limit_reached {
        break;
      }
    }
    report.truncated = report.entry_limit_reached
      || report.result_limit_reached
      || report.content_limit_reached
      || report.output_limit_reached
      || report.depth_limited > 0
      || report.skipped_symlinks > 0
      || report.skipped_directories > 0
      || report.skipped_binary > 0
      || report.skipped_large > 0
      || report.skipped_unreadable > 0
      || report.skipped_non_utf8 > 0;
    Ok(())
  }
}

// A conservative upper bound for a JSON string, including all escaped controls.
fn serde_json_size(text: &str) -> usize {
  text.len().saturating_mul(6).saturating_add(2)
}
