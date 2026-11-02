//! Client-only projections of durable execution events and reading navigation.
mod compaction;
mod display;
mod projection;
mod reading;
mod values;

use std::collections::{BTreeMap, HashMap, HashSet};

use agent_core::events::{AgentEvent, DecodedEvent, ModelIdentity, decode};
use agent_protocol::v1::Event;

pub use agent_core::events::TokenUsage;
pub use reading::{MessageAnchor, ReadingState, SearchHit};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
  Run,
  Tool,
  Compaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityStatus {
  Running,
  Succeeded,
  Failed,
  Cancelled,
  Uncertain,
}

#[derive(Debug, Clone)]
pub struct ActivityItem {
  pub run_id: String,
  pub kind: ActivityKind,
  pub title: String,
  pub target: String,
  pub status: ActivityStatus,
  pub started_at_ms: Option<i64>,
  pub finished_at_ms: Option<i64>,
  pub arguments: String,
  pub result: String,
  pub expanded: bool,
  pub model: Option<ModelIdentity>,
  key: String,
  order: (i64, u64),
  last_cursor: u64,
  model_cursor: u64,
}

impl ActivityItem {
  pub fn elapsed_ms(&self) -> Option<u64> {
    let elapsed = self.finished_at_ms?.saturating_sub(self.started_at_ms?);
    u64::try_from(elapsed).ok()
  }
}

#[derive(Debug, Default, Clone)]
pub struct ActivityState {
  items: Vec<ActivityItem>,
  selected: usize,
  seen: HashSet<(String, u64)>,
  usage: HashMap<(String, String), (u64, TokenUsage)>,
  compaction_events: HashMap<String, BTreeMap<u64, (i64, bool)>>,
  needs_sort: bool,
}

impl ActivityState {
  /// Returns false for duplicate, unknown or malformed events. History and live
  /// events share this path, including history delivered after subscription.
  pub fn apply(&mut self, event: &Event) -> bool {
    match decode(&event.kind, &event.payload_json) {
      Ok(DecodedEvent::Known(known)) => self.apply_typed(event, &known),
      // Unknown protocol extensions and historical journal payloads remain
      // forward compatible; the client does not infer execution state from JSON.
      _ => false,
    }
  }

  pub fn apply_typed(&mut self, metadata: &Event, event: &AgentEvent) -> bool {
    if !self.seen.insert((metadata.run_id.clone(), metadata.cursor)) {
      return false;
    }
    self.project(metadata, event);
    if self.needs_sort {
      let selected = self
        .items
        .get(self.selected)
        .map(|item| (item.run_id.clone(), item.key.clone()));
      self.items.sort_by_key(|item| item.order);
      if let Some((run, key)) = selected {
        self.selected = self
          .items
          .iter()
          .position(|item| item.run_id == run && item.key == key)
          .unwrap_or_default();
      }
      self.needs_sort = false;
    }
    true
  }

  pub fn replay<'a>(&mut self, events: impl IntoIterator<Item = &'a Event>) {
    let mut events: Vec<_> = events.into_iter().collect();
    events.sort_by_key(|event| event.cursor);
    for event in events {
      self.apply(event);
    }
  }

  pub fn items(&self) -> &[ActivityItem] {
    &self.items
  }

  pub fn selected(&self) -> usize {
    self.selected
  }

  pub fn select_next(&mut self) {
    self.selected = (self.selected + 1).min(self.items.len().saturating_sub(1));
  }

  pub fn select_previous(&mut self) {
    self.selected = self.selected.saturating_sub(1);
  }

  pub fn toggle_selected(&mut self) {
    if let Some(item) = self.items.get_mut(self.selected) {
      item.expanded = !item.expanded;
    }
  }

  pub fn token_usage(&self) -> TokenUsage {
    self
      .usage
      .values()
      .fold(TokenUsage::default(), |mut sum, (_, value)| {
        sum.input = sum.input.saturating_add(value.input);
        sum.output = sum.output.saturating_add(value.output);
        sum.cache_read = sum.cache_read.saturating_add(value.cache_read);
        sum.cache_write = sum.cache_write.saturating_add(value.cache_write);
        sum.total = sum.total.saturating_add(value.total);
        sum
      })
  }

  fn item(&mut self, event: &Event, key: &str, kind: ActivityKind) -> &mut ActivityItem {
    if let Some(index) = self
      .items
      .iter()
      .position(|item| item.run_id == event.run_id && item.key == key)
    {
      if (event.created_at_ms, event.cursor) < self.items[index].order {
        self.items[index].order = (event.created_at_ms, event.cursor);
        self.needs_sort = true;
      }
      return &mut self.items[index];
    }
    self.items.push(ActivityItem {
      run_id: event.run_id.clone(),
      key: key.into(),
      kind,
      title: String::new(),
      target: String::new(),
      status: ActivityStatus::Running,
      started_at_ms: None,
      finished_at_ms: None,
      arguments: String::new(),
      result: String::new(),
      expanded: false,
      model: None,
      order: (event.created_at_ms, event.cursor),
      last_cursor: 0,
      model_cursor: 0,
    });
    self.needs_sort = true;
    let index = self.items.len() - 1;
    &mut self.items[index]
  }
}
