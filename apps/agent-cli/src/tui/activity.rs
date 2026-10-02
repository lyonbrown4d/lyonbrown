use agent_app::{ActivityPhase, ActivityView, RuntimeSnapshot};
use agent_core::{StepId, TurnId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActivityTarget {
  pub turn_id: TurnId,
  pub step_id: StepId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityItem {
  pub target: ActivityTarget,
  pub phase: ActivityPhase,
  pub(crate) view: ActivityView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityFocus {
  List,
  Detail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityInspectorState {
  pub selected: Option<ActivityTarget>,
  pub focus: ActivityFocus,
  pub detail_scroll: u16,
  pub follow_latest: bool,
}

impl ActivityInspectorState {
  pub fn latest(activities: &[ActivityItem]) -> Self {
    Self {
      selected: activities.last().map(|item| item.target),
      focus: ActivityFocus::List,
      detail_scroll: 0,
      follow_latest: true,
    }
  }
}

pub(crate) fn project(snapshot: &RuntimeSnapshot) -> Vec<ActivityItem> {
  snapshot
    .turns
    .iter()
    .flat_map(|turn| {
      turn.activities.iter().map(move |activity| ActivityItem {
        target: ActivityTarget {
          turn_id: turn.turn_id,
          step_id: activity.step_id,
        },
        phase: activity.phase,
        view: activity.clone(),
      })
    })
    .collect()
}
