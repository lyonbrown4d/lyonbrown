use std::cell::{Ref, RefCell};

use crate::tui::{ActivityTarget, Locale, OverlayState, activity_detail};

use super::TuiState;

#[derive(Debug, Default)]
pub(super) struct ActivityDetailCache {
  entry: RefCell<Option<CachedDetail>>,
  format_count: RefCell<u64>,
}

#[derive(Debug)]
struct CachedDetail {
  generation: u64,
  target: Option<ActivityTarget>,
  locale: Locale,
  lines: Vec<String>,
}

impl ActivityDetailCache {
  pub(super) fn clear(&self) {
    *self.entry.borrow_mut() = None;
  }
}

impl TuiState {
  pub fn activity_detail(&self, locale: Locale) -> Ref<'_, Vec<String>> {
    let target = match &self.overlay {
      OverlayState::Activity(inspector) => inspector.selected,
      _ => None,
    };
    let cache_hit = self
      .activity_cache
      .entry
      .borrow()
      .as_ref()
      .is_some_and(|entry| {
        entry.generation == self.activity_generation
          && entry.target == target
          && entry.locale == locale
      });
    if !cache_hit {
      let lines = target
        .and_then(|target| self.activities.iter().find(|item| item.target == target))
        .map(|item| activity_detail::detail(item, locale))
        .unwrap_or_default();
      *self.activity_cache.entry.borrow_mut() = Some(CachedDetail {
        generation: self.activity_generation,
        target,
        locale,
        lines,
      });
      *self.activity_cache.format_count.borrow_mut() += 1;
    }
    Ref::map(self.activity_cache.entry.borrow(), |entry| {
      &entry.as_ref().expect("activity cache was populated").lines
    })
  }

  #[doc(hidden)]
  pub fn activity_projection_generation(&self) -> u64 {
    self.activity_generation
  }

  #[doc(hidden)]
  pub fn activity_detail_format_count(&self) -> u64 {
    *self.activity_cache.format_count.borrow()
  }

  pub(crate) const fn has_observed_snapshot(&self) -> bool {
    self.session_observed
  }
}
