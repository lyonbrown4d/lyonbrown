use std::{
  cell::RefCell,
  rc::Rc,
  sync::Arc,
  time::{Duration, Instant},
};

use super::*;
use uuid::Uuid;

#[derive(Default)]
struct Effects {
  moves: usize,
  steps: usize,
  text: String,
  changed: bool,
  hover_changes: bool,
}

struct Input(Rc<RefCell<Effects>>);

impl InputPort for Input {
  fn move_pointer(&mut self, _: i32, _: i32) -> Result<(), ()> {
    let mut effects = self.0.borrow_mut();
    effects.moves += 1;
    effects.changed = effects.hover_changes;
    Ok(())
  }
  fn step(&mut self, action: &DesktopAction) -> Result<(), ()> {
    if !matches!(action, DesktopAction::Move { .. }) {
      let mut effects = self.0.borrow_mut();
      effects.steps += 1;
      effects.changed = true;
    }
    Ok(())
  }
  fn character(&mut self, character: char) -> Result<(), ()> {
    let mut effects = self.0.borrow_mut();
    effects.text.push(character);
    effects.changed = true;
    Ok(())
  }
}

fn guard() -> ExecutionGuard {
  let leases = Arc::new(LeaseRegistry::default());
  let session = DesktopSession {
    id: Uuid::new_v4(),
    identity: DesktopIdentity {
      node_id: "test".into(),
      user_session: "test".into(),
    },
    scope: TargetScope::Window(42),
  };
  let token = leases
    .acquire(&session, Duration::from_secs(30))
    .expect("lease");
  ExecutionGuard {
    lease: leases.begin(&token).expect("execution"),
    control: OperationControl {
      cancellation: tokio_util::sync::CancellationToken::new(),
      deadline: Instant::now() + Duration::from_secs(10),
    },
  }
}

fn content(effects: &Rc<RefCell<Effects>>) -> Result<(), DesktopError> {
  if effects.borrow().changed {
    Err(DesktopError::StaleSnapshot)
  } else {
    Ok(())
  }
}

#[test]
fn changed_content_before_input_has_no_side_effects() {
  let effects = Rc::new(RefCell::new(Effects {
    changed: true,
    ..Effects::default()
  }));
  let outcome = execute_with_input(
    &DesktopAction::Click {
      x: 1,
      y: 1,
      button: MouseButton::Left,
    },
    &guard(),
    &mut Input(effects.clone()),
    || content(&effects),
    || Ok(()),
  );
  assert_eq!(
    outcome,
    ActionOutcome::Rejected(DesktopError::StaleSnapshot)
  );
  assert_eq!(effects.borrow().moves, 0);
  assert_eq!(effects.borrow().steps, 0);
}

#[test]
fn hover_change_stops_click_and_scroll_after_movement_without_retry() {
  for action in [
    DesktopAction::Click {
      x: 1,
      y: 1,
      button: MouseButton::Left,
    },
    DesktopAction::Scroll {
      x: 1,
      y: 1,
      axis: ScrollAxis::Vertical,
      lines: 1,
    },
  ] {
    let effects = Rc::new(RefCell::new(Effects {
      hover_changes: true,
      ..Effects::default()
    }));
    let outcome = execute_with_input(
      &action,
      &guard(),
      &mut Input(effects.clone()),
      || content(&effects),
      || Ok(()),
    );
    assert_eq!(
      outcome,
      ActionOutcome::Uncertain(DesktopError::StaleSnapshot)
    );
    assert_eq!(effects.borrow().moves, 1);
    assert_eq!(effects.borrow().steps, 0);
  }
}

#[test]
fn expected_content_changes_after_input_complete_and_do_not_truncate_text() {
  for action in [
    DesktopAction::Text {
      text: "ab中".into(),
    },
    DesktopAction::Key {
      key: DesktopKey::Enter,
    },
    DesktopAction::Click {
      x: 1,
      y: 1,
      button: MouseButton::Left,
    },
  ] {
    let effects = Rc::new(RefCell::new(Effects::default()));
    let outcome = execute_with_input(
      &action,
      &guard(),
      &mut Input(effects.clone()),
      || content(&effects),
      || Ok(()),
    );
    assert_eq!(outcome, ActionOutcome::Completed);
    assert!(effects.borrow().changed);
    if let DesktopAction::Text { text } = action {
      assert_eq!(effects.borrow().text, text);
    } else {
      assert_eq!(effects.borrow().steps, 1);
    }
  }
}

#[test]
fn session_failure_during_text_remains_uncertain_and_stops_remaining_input() {
  let effects = Rc::new(RefCell::new(Effects::default()));
  let outcome = execute_with_input(
    &DesktopAction::Text { text: "abc".into() },
    &guard(),
    &mut Input(effects.clone()),
    || content(&effects),
    || {
      if effects.borrow().text.is_empty() {
        Ok(())
      } else {
        Err(DesktopError::Locked)
      }
    },
  );
  assert_eq!(outcome, ActionOutcome::Uncertain(DesktopError::Locked));
  assert_eq!(effects.borrow().text, "a");
}

#[test]
fn pinned_process_reuse_rejects_before_input_and_stops_remaining_text() {
  let binding = crate::native::targets::TargetBinding::new(&DesktopTarget {
    scope: TargetScope::Window(42),
    process_id: Some(7),
    label: "private title".into(),
    bounds: Bounds {
      x: 0,
      y: 0,
      width: 2,
      height: 2,
    },
  })
  .expect("selected target");
  let effects = Rc::new(RefCell::new(Effects::default()));
  let outcome = execute_with_input(
    &DesktopAction::Click {
      x: 1,
      y: 1,
      button: MouseButton::Left,
    },
    &guard(),
    &mut Input(effects.clone()),
    || binding.check_identity(&TargetScope::Window(42), Some(8)),
    || Ok(()),
  );
  assert_eq!(
    outcome,
    ActionOutcome::Rejected(DesktopError::StaleSnapshot)
  );
  assert_eq!(effects.borrow().moves, 0);
  assert_eq!(effects.borrow().steps, 0);

  let outcome = execute_with_input(
    &DesktopAction::Text { text: "abc".into() },
    &guard(),
    &mut Input(effects.clone()),
    || binding.check_identity(&TargetScope::Window(42), Some(7)),
    || {
      let pid = if effects.borrow().text.is_empty() {
        7
      } else {
        8
      };
      binding.check_identity(&TargetScope::Window(42), Some(pid))
    },
  );
  assert_eq!(
    outcome,
    ActionOutcome::Uncertain(DesktopError::StaleSnapshot)
  );
  assert_eq!(effects.borrow().text, "a");
  assert_eq!(effects.borrow().steps, 0);
}
