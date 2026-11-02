use std::collections::HashSet;

use agent_protocol::v1::Event;

use super::{ActivityKind, ActivityState, ActivityStatus};

impl ActivityState {
  pub(super) fn compaction(&mut self, metadata: &Event, active: bool) {
    let markers = self
      .compaction_events
      .entry(metadata.run_id.clone())
      .or_default();
    markers.insert(metadata.cursor, (metadata.created_at_ms, active));
    let markers: Vec<_> = markers
      .iter()
      .map(|(&cursor, &(time, active))| (cursor, time, active))
      .collect();
    let expanded: HashSet<_> = self
      .items
      .iter()
      .filter(|item| {
        item.run_id == metadata.run_id && item.kind == ActivityKind::Compaction && item.expanded
      })
      .map(|item| item.key.clone())
      .collect();
    self
      .items
      .retain(|item| item.run_id != metadata.run_id || item.kind != ActivityKind::Compaction);
    // Group the typed start/end facts in durable cursor order. Rebuilding this
    // small projection handles any interleaving of history pages and live ends.
    let mut open = None;
    for (cursor, time, active) in markers {
      let marker = Event {
        run_id: metadata.run_id.clone(),
        cursor,
        created_at_ms: time,
        ..Event::default()
      };
      if active {
        let key = format!("compaction:{cursor}");
        let item = self.item(&marker, &key, ActivityKind::Compaction);
        item.title = "整理对话上下文".into();
        item.started_at_ms = Some(time);
        item.expanded = expanded.contains(&key);
        open = Some(self.items.len() - 1);
      } else {
        let index = open.take().unwrap_or_else(|| {
          self.item(
            &marker,
            &format!("compaction:{cursor}"),
            ActivityKind::Compaction,
          );
          self.items.len() - 1
        });
        let item = &mut self.items[index];
        item.title = "上下文整理已结束".into();
        // Inactive alone carries no outcome, so the UI does not claim success.
        item.status = ActivityStatus::Uncertain;
        item.finished_at_ms = Some(time);
      }
    }
    if self.run_ended(&metadata.run_id) {
      for item in &mut self.items {
        if item.run_id == metadata.run_id
          && item.kind == ActivityKind::Compaction
          && item.status == ActivityStatus::Running
        {
          item.status = ActivityStatus::Uncertain;
        }
      }
    }
    self.needs_sort = true;
  }
}
